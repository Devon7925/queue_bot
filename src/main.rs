use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::Hash,
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use itertools::Itertools;
use poise::{
    serenity_prelude::{
        self as serenity, Builder, ChannelId, ChannelType, CreateButton, CreateChannel,
        CreateMessage, EditMember, EditMessage, GuildId, Mentionable, UserId,
    },
    CreateReply,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use skillratings::{weng_lin::{WengLin, WengLinConfig, WengLinRating}, MultiTeamOutcome, MultiTeamRatingSystem, Rating, RatingPeriodSystem};

enum VoteType {
    None,
    Map,
    Result,
}

#[derive(Serialize, Deserialize, Clone)]
struct QueueConfiguration {
    team_size: u32,
    team_count: u32,
    category: Option<ChannelId>,
    queue_channels: Vec<ChannelId>,
    post_match_channel: Option<ChannelId>,
    maps: Vec<String>,
    map_vote_count: u32,
    default_player_data: PlayerData,
    skill_based_matchmaking: bool, // Indicates if skill-based matchmaking is enabled
}

impl Default for QueueConfiguration {
    fn default() -> Self {
        Self {
            team_size: 5,
            team_count: 2,
            category: None,
            queue_channels: vec![],
            post_match_channel: None,
            maps: vec![],
            map_vote_count: 0,
            default_player_data: PlayerData::default(), // Use default for PlayerData
            skill_based_matchmaking: true, // Default value for skill-based matchmaking
        }
    }
}

#[derive(Eq, PartialEq, Hash, Clone)]
enum MatchResult {
    Team(u32),
    Tie,
    Cancel,
}

impl std::fmt::Display for MatchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MatchResult::Team(num) => format!("Team {}", num),
                MatchResult::Tie => "Tie".to_string(),
                MatchResult::Cancel => "Cancel".to_string(),
            }
        )
    }
}

struct MatchData {
    result_votes: HashMap<UserId, MatchResult>,
    map_votes: HashMap<UserId, String>,
    channels: Vec<ChannelId>,
    members: Vec<Vec<UserId>>,
}

impl Default for MatchData {
    fn default() -> Self {
        Self {
            result_votes: HashMap::new(),
            channels: vec![],
            members: vec![],
            map_votes: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerQueueingConfig {
    cost_per_second: f32,
    cost_per_avg_mmr_differential: f32,
    acceptable_mmr_differential: f32,
    cost_per_mmr_range: f32,
    acceptable_mmr_range: f32,
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerData {
    rating: WengLinRating,
    queue_enter_time: Option<DateTime<Utc>>,
    player_queueing_config: PlayerQueueingConfig,
}

impl Default for PlayerData {
    fn default() -> Self {
        Self {
            rating: WengLinRating::default(), // Default skill rating and uncertainty
            queue_enter_time: None,
            player_queueing_config: PlayerQueueingConfig {
                cost_per_second: 1.0,
                cost_per_avg_mmr_differential: 0.04,
                acceptable_mmr_differential: 50.0,
                cost_per_mmr_range: 0.02,
                acceptable_mmr_range: 300.0,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Data {
    configuration: Mutex<QueueConfiguration>,
    #[serde(skip)]
    queued_players: Mutex<HashSet<UserId>>,
    #[serde(skip)]
    player_data: Mutex<HashMap<UserId, PlayerData>>,
    #[serde(skip)]
    match_data: Mutex<HashMap<u32, MatchData>>,
    #[serde(skip)]
    match_channels: Mutex<HashMap<ChannelId, u32>>,
    queue_idx: Mutex<u32>,
} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

async fn handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::VoiceStateUpdate { old, new } => {
            let mut player_added_to_queue = false;
            {
                let config = data.configuration.lock().unwrap();
                if let Some(old) = old {
                    if let Some(channel_id) = old.channel_id {
                        if config.queue_channels.contains(&channel_id) {
                            let mut queued_players = data.queued_players.lock().unwrap();
                            queued_players.remove(&old.user_id);
                        }
                    }
                }
                if let Some(channel_id) = new.channel_id {
                    if config.queue_channels.contains(&channel_id) {
                        {
                            let mut player_data = data.player_data.lock().unwrap();
                            player_data
                                .entry(new.user_id)
                                .or_insert_with(PlayerData::default).queue_enter_time = None;
                        }
                        {
                            let mut queued_players = data.queued_players.lock().unwrap();
                            queued_players.insert(new.user_id);
                            player_added_to_queue = true;
                        }
                    }
                }
            }
            if player_added_to_queue {
                try_matchmaking(data, ctx, new.guild_id.unwrap()).await?;
            }
        }
        serenity::FullEvent::InteractionCreate { interaction } => {
            if let Some(message_component) = interaction.as_message_component() {
                let required_votes = {
                    let config = data.configuration.lock().unwrap();
                    config.team_count * config.team_size / 2
                };
                let match_number = {
                    let match_channels = data.match_channels.lock().unwrap();
                    match_channels.get(&message_component.channel_id).cloned()
                };
                let Some(match_number) = match_number else {
                    return Ok(());
                };
                let mut vote_type = VoteType::None;
                {
                    let mut match_data = data.match_data.lock().unwrap();
                    if let Some(map) = message_component.data.custom_id.strip_prefix("map_") {
                        match_data
                            .get_mut(&match_number)
                            .unwrap()
                            .map_votes
                            .insert(message_component.user.id, map.to_string());
                        vote_type = VoteType::Map;
                    }
                    if let Some(team_data) = message_component.data.custom_id.strip_prefix("team_")
                    {
                        let team_number: u32 = team_data.parse()?;
                        match_data
                            .get_mut(&match_number)
                            .unwrap()
                            .result_votes
                            .insert(message_component.user.id, MatchResult::Team(team_number));
                        vote_type = VoteType::Result;
                    }
                    if message_component.data.custom_id.eq_ignore_ascii_case("tie") {
                        match_data
                            .get_mut(&match_number)
                            .unwrap()
                            .result_votes
                            .insert(message_component.user.id, MatchResult::Tie);
                        vote_type = VoteType::Result;
                    }
                    if message_component
                        .data
                        .custom_id
                        .eq_ignore_ascii_case("cancel")
                    {
                        match_data
                            .get_mut(&match_number)
                            .unwrap()
                            .result_votes
                            .insert(message_component.user.id, MatchResult::Cancel);
                        vote_type = VoteType::Result;
                    }
                }
                if matches!(vote_type, VoteType::Map) {
                    let mut vote_result = None;
                    let mut content = {
                        let match_data = data.match_data.lock().unwrap();
                        let mut votes: HashMap<String, u32> = HashMap::new();
                        for (_user, vote) in match_data.get(&match_number).unwrap().map_votes.iter()
                        {
                            let current_votes = votes.get(vote).unwrap_or(&0);
                            votes.insert(vote.clone(), current_votes + 1);
                        }
                        let mut content = String::new();
                        for (vote_type, count) in votes {
                            content += format!("{}: {}\n", vote_type, count).as_str();
                            if count > required_votes {
                                vote_result = Some(vote_type);
                            }
                        }
                        content
                    };
                    if let Some(vote_result) = vote_result {
                        ctx.http
                            .clone()
                            .get_message(message_component.channel_id, message_component.message.id)
                            .await?
                            .edit(ctx.http.clone(), EditMessage::new().components(vec![]))
                            .await?;
                        content = format!("# Map: {}", vote_result);
                    }
                    ctx.http
                        .clone()
                        .get_message(message_component.channel_id, message_component.message.id)
                        .await?
                        .edit(ctx.http.clone(), EditMessage::new().content(content))
                        .await?;
                }
                if matches!(vote_type, VoteType::Result) {
                    let mut vote_result = None;
                    let content = {
                        let match_data = data.match_data.lock().unwrap();
                        let mut votes: HashMap<MatchResult, u32> = HashMap::new();
                        for (_user, vote) in
                            match_data.get(&match_number).unwrap().result_votes.iter()
                        {
                            let current_votes = votes.get(&vote).unwrap_or(&0);
                            votes.insert(vote.clone(), current_votes + 1);
                        }
                        let mut content = String::new();
                        for (vote_type, count) in votes {
                            content += format!("{}: {}\n", vote_type, count).as_str();
                            if count > required_votes {
                                vote_result = Some(vote_type);
                            }
                        }
                        content
                    };
                    if let Some(vote_result) = vote_result {
                        let post_match_channel = data
                            .configuration
                            .lock()
                            .unwrap()
                            .post_match_channel
                            .clone();
                        let (channels, players) = {
                            let mut match_data = data.match_data.lock().unwrap();
                            let match_data = match_data.remove(&match_number).unwrap();
                            (match_data.channels, match_data.members)
                        };

                        apply_match_results(data, vote_result, &players);
                        
                        let guild_id = message_component.guild_id.unwrap();
                        if let Some(post_match_channel) = post_match_channel {
                            for player in players.iter().flat_map(|t| t) {
                                ctx.http
                                    .get_member(guild_id, *player)
                                    .await?
                                    .edit(
                                        ctx.http.clone(),
                                        EditMember::new().voice_channel(post_match_channel),
                                    )
                                    .await?;
                            }
                        }
                        for channel in channels {
                            data.match_channels.lock().unwrap().remove(&channel);
                            ctx.http.delete_channel(channel, None).await?;
                        }
                    }
                    ctx.http
                        .clone()
                        .get_message(message_component.channel_id, message_component.message.id)
                        .await?
                        .edit(ctx.http.clone(), EditMessage::new().content(content))
                        .await?;
                }
                message_component.defer(ctx.http.clone()).await?;
            }
        }
        serenity::FullEvent::Ratelimit { .. } => {
            println!("Rate limited")
        }
        _ => {}
    }
    Ok(())
}

fn apply_match_results(
    data: &Data,
    result: MatchResult,
    players: &Vec<Vec<UserId>>,
) {
    let rating_config: WengLinConfig = WengLinConfig::default();
    if matches!(result, MatchResult::Cancel) {
        return;
    }
    let system = <WengLin as MultiTeamRatingSystem>::new(rating_config);
    let mut player_data = data.player_data.lock().unwrap();
    let outcome = players.iter().enumerate().map(|(team_idx, team)| {
        (team.iter().map(|id| player_data.get(id).unwrap().rating).collect_vec(), 
        MultiTeamOutcome::new(match result {
            MatchResult::Team(idx) if idx == team_idx as u32 => 1,
            MatchResult::Team(_) => 2,
            MatchResult::Tie => 1,
            MatchResult::Cancel => panic!("Invalid state")
        }))
    }).collect_vec();
    let result = MultiTeamRatingSystem::rate(&system, outcome.iter().map(|(t, o)| (t.as_slice(), o.clone())).collect_vec().as_slice());
    for (team_idx, team) in players.iter().enumerate() {
        for (player_idx, player) in team.iter().enumerate() {
            player_data.get_mut(player).unwrap().rating = result.get(team_idx).unwrap().get(player_idx).unwrap().clone();
        }
    }
}

async fn try_matchmaking(
    data: &Data,
    ctx: &serenity::Context,
    guild_id: GuildId,
) -> Result<(), Error> {
    // Extract team size, team count, and skill-based matchmaking flag from the configuration
    let (team_count, team_size, skill_based) = {
        let configuration = data.configuration.lock().unwrap();
        let queued_players = data.queued_players.lock().unwrap();
        if (queued_players.len() as u32) < configuration.team_count * configuration.team_size {
            return Ok(());
        }
        (configuration.team_count, configuration.team_size, configuration.skill_based_matchmaking)
    };
    let cache_http = ctx.http.clone();
    let config = {
        let config = data.configuration.lock().unwrap();
        config.clone()
    };
    let Some(category) = config.category else {
        return Err(Error::from("No category"));
    };
    let queued_players = data.queued_players.lock().unwrap().clone();
    let mut members = queued_players.iter().cloned().collect_vec();

    if skill_based {
        // Sort players by rating in descending order if skill-based matchmaking is enabled
        let player_data = data.player_data.lock().unwrap();
        members.sort_by_key(|id| {
            let player = player_data.get(id).unwrap_or(&config.default_player_data);
            player.rating.rating as i32
        });
    }

    // Split players into balanced teams using round-robin method
    let mut teams: Vec<Vec<UserId>> = vec![vec![]; team_count as usize];
    for (i, player) in members.into_iter().enumerate() {
        teams[i % team_count as usize].push(player);
    }

    let members = queued_players
        .iter()
        .chunks(team_size as usize)
        .into_iter()
        .map(|c| c.cloned().collect_vec())
        .collect_vec();
    let new_idx = {
        let mut queue_idx = data.queue_idx.lock().unwrap();
        *queue_idx += 1;
        *queue_idx
    };
    let match_channel = CreateChannel::new(format!("match-{}", new_idx))
        .category(category.clone())
        .execute(cache_http.clone(), guild_id)
        .await?;
    let mut vc_channels = vec![];
    for i in 0..team_count {
        vc_channels.push(
            CreateChannel::new(format!("Team {} - #{}", i + 1, new_idx))
                .category(category.clone())
                .kind(ChannelType::Voice)
                .execute(cache_http.clone(), guild_id)
                .await
                .unwrap(),
        );
    }
    let mut members_message = String::new();
    members_message += format!("# Queue#{}\n", new_idx).as_str();
    for (team_idx, team) in members.iter().enumerate() {
        members_message += format!("## Team {}\n", team_idx + 1).as_str();
        for player in team {
            members_message += format!("{}\n", player.mention()).as_str();
        }
    }
    match_channel
        .send_message(
            cache_http.clone(),
            CreateMessage::default().content(members_message),
        )
        .await?;
    if config.map_vote_count > 0 {
        let mut map_vote_message = CreateMessage::default().content("# Map Vote");
        let mut map_pool = config.maps.clone();
        for _ in 0..config.map_vote_count {
            let num = rand::thread_rng().gen_range(0..map_pool.len());
            let rand_map = map_pool.remove(num);
            map_vote_message = map_vote_message.button(
                CreateButton::new(format!("map_{}", rand_map).clone())
                    .label(rand_map)
                    .style(serenity::ButtonStyle::Secondary),
            );
        }
        match_channel
            .send_message(cache_http.clone(), map_vote_message)
            .await?;
    } else if config.maps.len() > 0 {
        let num = rand::thread_rng().gen_range(0..config.maps.len());
        let chosen_map = config.maps.get(num).unwrap().clone();
        let map_vote_message = CreateMessage::default().content(format!("# Map: {}", chosen_map));
        match_channel
            .send_message(cache_http.clone(), map_vote_message)
            .await?;
    }
    let mut result_message = CreateMessage::default();
    for i in 0..team_count {
        result_message = result_message.button(
            CreateButton::new(format!("team_{}", i + 1))
                .label(format!("Team {}", i+1))
                .style(serenity::ButtonStyle::Primary),
        )
    }
    match_channel
        .send_message(
            cache_http.clone(),
            result_message
                .button(
                    CreateButton::new("tie")
                        .label("Tie")
                        .style(serenity::ButtonStyle::Secondary),
                )
                .button(
                    CreateButton::new("cancel")
                        .label("Cancel")
                        .style(serenity::ButtonStyle::Danger),
                ),
        )
        .await?;
    {
        let mut channels = data.match_channels.lock().unwrap();
        channels.insert(match_channel.id, new_idx);
    }
    {
        let mut match_data = data.match_data.lock().unwrap();
        let mut channels = vec![match_channel.id];
        channels.extend(vc_channels.iter().map(|c| c.id));
        match_data.insert(
            new_idx,
            MatchData {
                result_votes: HashMap::new(),
                channels,
                members: members.clone(),
                map_votes: HashMap::new(),
            },
        );
    }
    {
        for (team_idx, team) in members.iter().enumerate() {
            for player in team {
                guild_id
                    .member(ctx.http.clone(), player)
                    .await?
                    .edit(
                        cache_http.clone(),
                        EditMember::new().voice_channel(vc_channels.get(team_idx).unwrap().clone()),
                    )
                    .await?;
                data.queued_players.lock().unwrap().remove(player);
                data.player_data.lock().unwrap().get_mut(player).unwrap().queue_enter_time = None;
            }
        }
    }
    Ok(())
}

/// Command to set a players rating (for test)
#[poise::command(slash_command, prefix_command)]
async fn set_rating(
    ctx: Context<'_>,
    #[description = "User to set rating for"] user: serenity::UserId,
    #[description = "New rating value"] rating: f64,
) -> Result<(), Error> {
    {
        // Lock player data and set the new rating
        let mut player_data = ctx.data().player_data.lock().unwrap();
        let config = ctx.data().configuration.lock().unwrap();
        let player = player_data
            .entry(user)
            .or_insert(config.default_player_data.clone());
        player.rating.rating = rating;
    }
    // Send confirmation message
    let response = format!("Set rating of {} to {}", user.mention(), rating);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Command to get a player's rating
#[poise::command(slash_command, prefix_command)]
async fn get_rating(
    ctx: Context<'_>,
    #[description = "User to get rating for"] user: Option<serenity::UserId>,
) -> Result<(), Error> {
    let user = user.unwrap_or(ctx.author().id);
    let rating = {
        // Lock player data and retrieve the rating
        let player_data = ctx.data().player_data.lock().unwrap();
        let config = ctx.data().configuration.lock().unwrap();
        player_data
            .get(&user)
            .unwrap_or(&config.default_player_data)
            .rating
            .rating
    };
    // Send message with the rating
    let response = format!("{}'s rating is {}", user.mention(), rating);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays or sets team size
#[poise::command(slash_command, prefix_command, rename = "team_size")]
async fn configure_team_size(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 1]
    new_value: Option<u32>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.team_size = new_value;
        }
        let response = format!("Team size set to {}", new_value);
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!("Team size is currently {}", data_lock.team_size)
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Displays or sets team count
#[poise::command(slash_command, prefix_command, rename = "team_count")]
async fn configure_team_count(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 1]
    new_value: Option<u32>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.team_count = new_value;
        }
        let response = format!("Team count set to {}", new_value);
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!("Team count is currently {}", data_lock.team_count)
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Displays or sets queue category
#[poise::command(slash_command, prefix_command, rename = "queue_category")]
async fn configure_queue_category(
    ctx: Context<'_>,
    #[description = "Queue category"]
    #[channel_types("Category")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        if new_value.clone().category().is_none() {
            let response = format!(
                "Channel {} is not a category.",
                new_value.clone().to_string()
            );
            ctx.send(CreateReply::default().content(response).ephemeral(true))
                .await?;
            return Ok(());
        }
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.category = Some(new_value.id().clone());
            format!("Queue category set to {}", new_value.to_string())
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!(
                "Queue category is currently {}",
                data_lock
                    .category
                    .as_ref()
                    .map(|c| format!("{}", c.mention()))
                    .unwrap_or("not set".to_string())
            )
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Displays or adds queue channels
#[poise::command(slash_command, prefix_command, rename = "queue_channels")]
async fn configure_queue_channels(
    ctx: Context<'_>,
    #[description = "New queue channel"]
    #[channel_types("Voice")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.queue_channels.push(new_value.id());
            format!("{} added as queue channel", new_value.to_string())
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!(
                "Queue channels are {}",
                data_lock
                    .queue_channels
                    .iter()
                    .map(|c| c.mention())
                    .join(", ")
            )
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

// Displays or adds maps
#[poise::command(slash_command, prefix_command, rename = "maps")]
async fn configure_maps(
    ctx: Context<'_>,
    #[description = "New map"] new_value: Option<String>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.maps.push(new_value.clone());
            format!("{} added as map", new_value)
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!(
                "Maps are {}",
                data_lock
                    .queue_channels
                    .iter()
                    .map(|c| c.mention())
                    .join(", ")
            )
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Displays or sets number of maps for the vote
#[poise::command(slash_command, prefix_command, rename = "map_vote_count")]
async fn configure_map_vote_count(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<u32>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.map_vote_count = new_value;
        }
        let response = format!("Map vote count set to {}", new_value);
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!("Map vote count is currently {}", data_lock.map_vote_count)
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Sets the channel to move members to after the end of the game
#[poise::command(slash_command, prefix_command, rename = "post_match_channel")]
async fn configure_post_match_channel(
    ctx: Context<'_>,
    #[description = "Post match channel"]
    #[channel_types("Voice")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.post_match_channel = Some(new_value.id());
            format!("Post match channel changed to {}", new_value.to_string())
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!(
                "Post match channel is {}",
                data_lock
                    .category
                    .as_ref()
                    .map(|c| format!("{}", c.mention()))
                    .unwrap_or("not set".to_string())
            )
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Exports configuration
#[poise::command(slash_command, prefix_command)]
async fn export_config(ctx: Context<'_>) -> Result<(), Error> {
    let config = serde_json::to_string_pretty(ctx.data())?;
    let response = format!("Configuration: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays or sets queue category
#[poise::command(slash_command, prefix_command)]
async fn list_queued(ctx: Context<'_>) -> Result<(), Error> {
    let response = {
        let data_lock = ctx.data().queued_players.lock().unwrap();
        format!(
            "Queued players: {}",
            data_lock.iter().map(|c| c.mention()).join(", ")
        )
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays your or another user's account creation date
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS",
    subcommands(
        "configure_team_size",
        "configure_queue_category",
        "configure_queue_channels",
        "configure_team_count",
        "configure_post_match_channel",
        "configure_maps",
        "configure_map_vote_count"
    )
)]
async fn configure(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Sets the channel to move members to after the end of the game
#[poise::command(slash_command, prefix_command)]
async fn stats(
    ctx: Context<'_>,
    #[description = "User to get stats for"] user: Option<serenity::UserId>,
) -> Result<(), Error> {
    let user = user.unwrap_or(ctx.author().id);
    let rating = {
        let mut player_data = ctx.data().player_data.lock().unwrap();
        let config = ctx.data().configuration.lock().unwrap();
        player_data
            .entry(user)
            .or_insert(config.default_player_data.clone())
            .rating
    };
    let response = format!("{}'s mmr is {}, with uncertainty {}", user.mention(), rating.rating, rating.uncertainty);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

#[poise::command(prefix_command)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_BOT_TOKEN").expect("missing DISCORD_BOT_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            event_handler: |ctx, event, framework, data| {
                Box::pin(handler(ctx, event, framework, data))
            },
            commands: vec![register(), configure(), export_config(), list_queued(), stats(), set_rating(), get_rating()],
            ..Default::default()
        })
        .setup(|_ctx, _ready, _framework| {
            Box::pin(async move {
                let config_data: Option<Data> =
                    fs::read_to_string("config.json").ok().map(|read| {
                        serde_json::from_str(read.as_str()).expect("Failed to parse config file")
                    });
                if let Some(data) = config_data {
                    return Ok(data);
                }
                Ok(Data {
                    configuration: Mutex::new(QueueConfiguration::default()),
                    queue_idx: Mutex::new(0),
                    queued_players: Mutex::new(HashSet::new()),
                    match_channels: Mutex::new(HashMap::new()),
                    player_data: Mutex::new(HashMap::new()),
                    match_data: Mutex::new(HashMap::new()),
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}
