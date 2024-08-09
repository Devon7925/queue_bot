use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    hash::Hash,
    io::prelude::*,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use itertools::{Itertools, MinMaxResult};
use poise::{
    serenity_prelude::{
        self as serenity, futures::future, Builder, CacheHttp, ChannelId, ChannelType, CreateActionRow, CreateAllowedMentions, CreateButton, CreateChannel, CreateInteractionResponseMessage, CreateMessage, EditMember, EditMessage, GuildId, Http, Mentionable, RoleId, User, UserId
    },
    CreateReply,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use skillratings::{
    weng_lin::{WengLin, WengLinConfig, WengLinRating},
    MultiTeamOutcome, MultiTeamRatingSystem,
};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Data {
    configuration: Mutex<QueueConfiguration>,
    #[serde(default)]
    queued_players: Mutex<HashSet<UserId>>,
    #[serde(default)]
    in_game_players: Mutex<HashSet<UserId>>,
    #[serde(default)]
    player_data: Mutex<HashMap<UserId, DerivedPlayerData>>,
    #[serde(default)]
    match_data: Mutex<HashMap<u32, MatchData>>,
    #[serde(default)]
    match_channels: Mutex<HashMap<ChannelId, u32>>,
    #[serde(default)]
    group_data: Mutex<HashMap<Uuid, QueueGroup>>,
    #[serde(default)]
    player_bans: Mutex<HashMap<UserId, BanData>>,
    #[serde(default)]
    leaver_data: Mutex<HashMap<UserId, u32>>,
    queue_idx: Mutex<u32>,
} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Arc<Data>, Error>;

impl Default for Data {
    fn default() -> Self {
        Self {
            configuration: Mutex::new(QueueConfiguration::default()),
            queue_idx: Mutex::new(0),
            queued_players: Mutex::new(HashSet::new()),
            match_channels: Mutex::new(HashMap::new()),
            player_data: Mutex::new(HashMap::new()),
            match_data: Mutex::new(HashMap::new()),
            in_game_players: Mutex::new(HashSet::new()),
            group_data: Mutex::new(HashMap::new()),
            player_bans: Mutex::new(HashMap::new()),
            leaver_data: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct BanData {
    end_time: Option<DateTime<Utc>>,
    reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct QueueGroup {
    players: HashSet<UserId>,
}

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
    map_vote_time: u32,
    leaver_verification_time: u32,
    default_player_data: PlayerData,
    maximum_queue_cost: f32,
    game_categories: HashMap<String, Vec<RoleId>>,
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
            map_vote_time: 0,
            leaver_verification_time: 30,
            default_player_data: PlayerData::default(),
            maximum_queue_cost: 50.0,
            game_categories: HashMap::new(),
        }
    }
}

#[derive(Eq, PartialEq, Hash, Clone, Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
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
    cost_per_avg_mmr_differential: f32,
    acceptable_mmr_differential: f32,
    cost_per_mmr_range: f32,
    acceptable_mmr_range: f32,
    wrong_game_category_cost: HashMap<String, f32>,
}

#[derive(Serialize, Deserialize, Clone)]
struct DerivedPlayerQueueingConfig {
    cost_per_avg_mmr_differential: Option<f32>,
    acceptable_mmr_differential: Option<f32>,
    cost_per_mmr_range: Option<f32>,
    acceptable_mmr_range: Option<f32>,
    wrong_game_category_cost: Option<HashMap<String, f32>>,
}

impl DerivedPlayerQueueingConfig {
    fn derive(&self, base: &PlayerQueueingConfig) -> PlayerQueueingConfig {
        PlayerQueueingConfig {
            cost_per_avg_mmr_differential: self
                .cost_per_avg_mmr_differential
                .unwrap_or(base.cost_per_avg_mmr_differential),
            acceptable_mmr_differential: self
                .acceptable_mmr_differential
                .unwrap_or(base.acceptable_mmr_differential),
            cost_per_mmr_range: self.cost_per_mmr_range.unwrap_or(base.cost_per_mmr_range),
            acceptable_mmr_range: self
                .acceptable_mmr_range
                .unwrap_or(base.acceptable_mmr_range),
            wrong_game_category_cost: self
                .wrong_game_category_cost
                .clone()
                .unwrap_or(base.wrong_game_category_cost.clone()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerData {
    rating: WengLinRating,
    player_queueing_config: PlayerQueueingConfig,
}

impl Default for PlayerData {
    fn default() -> Self {
        Self {
            rating: WengLinRating::default(),
            player_queueing_config: PlayerQueueingConfig {
                cost_per_avg_mmr_differential: 0.04,
                acceptable_mmr_differential: 50.0,
                cost_per_mmr_range: 0.02,
                acceptable_mmr_range: 300.0,
                wrong_game_category_cost: HashMap::new(),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct DerivedPlayerData {
    rating: Option<WengLinRating>,
    queue_enter_time: Option<DateTime<Utc>>,
    party: Option<Uuid>,
    player_queueing_config: DerivedPlayerQueueingConfig,
    game_categories: HashMap<String, Vec<usize>>,
}

impl Default for DerivedPlayerData {
    fn default() -> Self {
        Self {
            rating: None,
            queue_enter_time: None,
            party: None,
            player_queueing_config: DerivedPlayerQueueingConfig {
                cost_per_avg_mmr_differential: None,
                acceptable_mmr_differential: None,
                cost_per_mmr_range: None,
                acceptable_mmr_range: None,
                wrong_game_category_cost: None,
            },
            game_categories: HashMap::new(),
        }
    }
}

async fn try_queue_player(
    data: Arc<Data>,
    user_id: UserId,
    http: impl CacheHttp + Clone,
    guild_id: GuildId,
) -> Result<(), String> {
    update_bans(data.clone());
    let game_categories = {
        let config = data.configuration.lock().unwrap();
        config.game_categories.clone()
    };
    let user = user_id.to_user(http.clone()).await.unwrap();

    let player_categories: HashMap<String, Vec<usize>> = future::join_all(
        game_categories
            .iter()
            .map(|(category_name, category_roles)| async {
                (
                    category_name.clone(),
                    future::join_all(category_roles.iter().map(|role| async {
                        user.has_role(http.clone(), guild_id, *role).await.unwrap()
                    }))
                    .await
                    .iter()
                    .enumerate()
                    .filter(|(_, has_role)| **has_role)
                    .map(|(idx, _)| idx)
                    .collect_vec(),
                )
            }),
    )
    .await
    .into_iter()
    .collect();
    let mut player_data = data.player_data.lock().unwrap();
    player_data
        .entry(user_id)
        .or_insert(DerivedPlayerData::default())
        .game_categories = player_categories;
    let mut queued_players = data.queued_players.lock().unwrap();
    if let Some(player_ban) = data.player_bans.lock().unwrap().get(&user_id) {
        if let Some(ban_reason) = player_ban.reason.clone() {
            return Err(format!(
                "Cannot queue because you're banned for {}",
                ban_reason
            ));
        }
        return Err("Cannot queue because you're banned".to_string());
    }
    if data.in_game_players.lock().unwrap().contains(&user_id) {
        return Err("Cannot queue while in game!".to_string());
    }
    player_data
        .entry(user_id)
        .or_insert(DerivedPlayerData::default())
        .queue_enter_time = Some(chrono::offset::Utc::now());
    queued_players.insert(user_id);
    Ok(())
}

async fn handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Arc<Data>, Error>,
    data: Arc<Data>,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { .. } => {
            println!("Ready")
        }
        serenity::FullEvent::VoiceStateUpdate { old, new } => {
            let mut player_added_to_queue = false;
            {
                {
                    let config = data.configuration.lock().unwrap();
                    if let Some(old) = old {
                        if let Some(channel_id) = old.channel_id {
                            if config.queue_channels.contains(&channel_id) {
                                let mut player_data = data.player_data.lock().unwrap();
                                let mut queued_players = data.queued_players.lock().unwrap();
                                player_data
                                    .entry(new.user_id)
                                    .or_insert(DerivedPlayerData::default())
                                    .queue_enter_time = None;
                                queued_players.remove(&old.user_id);
                            }
                        }
                    }
                }
                let try_queueing = {
                    let config = data.configuration.lock().unwrap();
                    if let Some(channel_id) = new.channel_id {
                        config.queue_channels.contains(&channel_id)
                    } else {
                        false
                    }
                };

                if try_queueing {
                    match try_queue_player(
                        data.clone(),
                        new.user_id,
                        ctx.http.clone(),
                        new.guild_id.unwrap(),
                    )
                    .await
                    {
                        Ok(()) => {
                            player_added_to_queue = true;
                        }
                        Err(reason) => {
                            new.user_id
                                .direct_message(ctx, CreateMessage::new().content(reason))
                                .await?;
                        }
                    }
                }
            }
            if player_added_to_queue {
                if let Some(delay) =
                    try_matchmaking(data.clone(), ctx.http.clone(), new.guild_id.unwrap()).await?
                {
                    tokio::time::sleep(Duration::from_secs(delay as u64)).await;
                    try_matchmaking(data.clone(), ctx.http.clone(), new.guild_id.unwrap()).await?;
                }
            }
        }
        serenity::FullEvent::InteractionCreate { interaction } => {
            if let Some(message_component) = interaction.as_message_component() {
                let required_votes = {
                    let config = data.configuration.lock().unwrap();
                    config.team_count * config.team_size / 2 + 1
                };
                let match_number = {
                    let match_channels = data.match_channels.lock().unwrap();
                    match_channels.get(&message_component.channel_id).cloned()
                };
                if let Some(match_number) = match_number {
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
                        if let Some(team_data) =
                            message_component.data.custom_id.strip_prefix("team_")
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
                            for (_user, vote) in
                                match_data.get(&match_number).unwrap().map_votes.iter()
                            {
                                let current_votes = votes.get(vote).unwrap_or(&0);
                                votes.insert(vote.clone(), current_votes + 1);
                            }
                            let mut content = String::new();
                            for (vote_type, count) in votes {
                                content += format!("{}: {}\n", vote_type, count).as_str();
                                if count >= required_votes {
                                    vote_result = Some(vote_type);
                                }
                            }
                            content
                        };
                        if let Some(vote_result) = vote_result {
                            ctx.http
                                .clone()
                                .get_message(
                                    message_component.channel_id,
                                    message_component.message.id,
                                )
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
                                if count >= required_votes {
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
                                log_match_results(
                                    data.clone(),
                                    &vote_result,
                                    &match_data,
                                    match_number,
                                );
                                (match_data.channels, match_data.members)
                            };

                            apply_match_results(data.clone(), vote_result, &players);

                            let guild_id = message_component.guild_id.unwrap();
                            if let Some(post_match_channel) = post_match_channel {
                                for player in players.iter().flat_map(|t| t) {
                                    data.in_game_players.lock().unwrap().remove(player);
                                    ctx.http
                                        .get_member(guild_id, *player)
                                        .await?
                                        .edit(
                                            ctx.http.clone(),
                                            EditMember::new().voice_channel(post_match_channel),
                                        )
                                        .await
                                        .ok();
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
                if let Some(party_id) = message_component.data.custom_id.strip_prefix("join_party_")
                {
                    let party_uuid = serde_json::from_str::<Uuid>(party_id).unwrap();
                    let group_members = {
                        let mut group_data = data.group_data.lock().unwrap();
                        let party = group_data.get_mut(&party_uuid);
                        if let Some(party) = party {
                            party.players.insert(message_component.user.id);
                            Some(party.players.clone())
                        } else {
                            None
                        }
                    };
                    let Some(group_members) = group_members else {
                        message_component
                            .create_response(
                                ctx,
                                serenity::CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("Party no longer exists.")),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    let old_party = {
                        let mut player_data = data.player_data.lock().unwrap();
                        let player_data = player_data
                            .entry(message_component.user.id)
                            .or_insert(DerivedPlayerData::default());
                        let old_party = player_data.party;
                        player_data.party = Some(party_uuid);
                        old_party
                    };
                    if let Some(old_party) = old_party {
                        if old_party != party_uuid {
                            leave_party(
                                data,
                                &message_component.user.id,
                                Arc::new(ctx.http()),
                                old_party,
                            )
                            .await?;
                        }
                    }

                    for group_member in group_members {
                        if group_member == message_component.user.id {
                            continue;
                        }
                        group_member
                            .direct_message(
                                ctx,
                                CreateMessage::new().content(format!(
                                    "{} joined your party!",
                                    message_component.user.id.mention()
                                )),
                            )
                            .await?;
                    }
                    message_component.message.delete(ctx).await?;
                    message_component
                        .create_response(
                            ctx,
                            serenity::CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("Joined party!"))
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    return Ok(());
                }
                if let Some(party_id) = message_component
                    .data
                    .custom_id
                    .strip_prefix("reject_party_")
                {
                    let group_members = {
                        let group_data = data.group_data.lock().unwrap();
                        let party =
                            group_data.get(&serde_json::from_str::<Uuid>(party_id).unwrap());
                        if let Some(party) = party {
                            Some(party.players.clone())
                        } else {
                            None
                        }
                    };
                    let Some(group_members) = group_members else {
                        message_component
                            .create_response(
                                ctx,
                                serenity::CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("Party no longer exists.")),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    for group_member in group_members {
                        if group_member == message_component.user.id {
                            continue;
                        }
                        group_member
                            .direct_message(
                                ctx,
                                CreateMessage::new().content(format!(
                                    "{} rejected your party invite",
                                    message_component.user.id.mention()
                                )),
                            )
                            .await?;
                    }
                    message_component.message.delete(ctx).await?;
                    message_component
                        .create_response(
                            ctx,
                            serenity::CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("Rejected party invite."))
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    return Ok(());
                }
                if let Some(non_leaver_id) = message_component.data.custom_id.strip_prefix("leaver_check_") {
                    let player = UserId::new(non_leaver_id.parse::<u64>().unwrap());
                    if message_component.user.id != player {
                        message_component.create_response(ctx, serenity::CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(format!("You aren't the right player silly :P"))
                                .ephemeral(true),
                        )).await?;
                        return Ok(())
                    }
                    message_component.message.delete(ctx).await?;
                    message_component.create_response(ctx, serenity::CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("You are no longer marked as a leaver."))
                            .ephemeral(true),
                    )).await?;
                    return Ok(())
                }
            }
        }
        serenity::FullEvent::Ratelimit { .. } => {
            println!("Rate limited")
        }
        _ => {}
    }
    Ok(())
}

fn log_match_results(_data: Arc<Data>, result: &MatchResult, match_data: &MatchData, number: u32) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("games.log")
        .unwrap();
    if let Err(e) = writeln!(
        file,
        "match #{}:{:?}\nresult:{}",
        number, match_data, result
    ) {
        eprintln!("Couldn't write to file: {}", e);
    }
}

fn apply_match_results(data: Arc<Data>, result: MatchResult, players: &Vec<Vec<UserId>>) {
    let rating_config: WengLinConfig = WengLinConfig::default();
    if matches!(result, MatchResult::Cancel) {
        return;
    }
    let system = <WengLin as MultiTeamRatingSystem>::new(rating_config);
    let mut player_data = data.player_data.lock().unwrap();
    let config = data.configuration.lock().unwrap();
    let outcome = players
        .iter()
        .enumerate()
        .map(|(team_idx, team)| {
            (
                team.iter()
                    .map(|id| {
                        player_data
                            .get(id)
                            .unwrap()
                            .rating
                            .unwrap_or(config.default_player_data.rating)
                    })
                    .collect_vec(),
                MultiTeamOutcome::new(match result {
                    MatchResult::Team(idx) if idx == team_idx as u32 => 1,
                    MatchResult::Team(_) => 2,
                    MatchResult::Tie => 1,
                    MatchResult::Cancel => panic!("Invalid state"),
                }),
            )
        })
        .collect_vec();
    let result = MultiTeamRatingSystem::rate(
        &system,
        outcome
            .iter()
            .map(|(t, o)| (t.as_slice(), o.clone()))
            .collect_vec()
            .as_slice(),
    );
    for (team_idx, team) in players.iter().enumerate() {
        for (player_idx, player) in team.iter().enumerate() {
            player_data.get_mut(player).unwrap().rating = Some(
                result
                    .get(team_idx)
                    .unwrap()
                    .get(player_idx)
                    .unwrap()
                    .clone(),
            );
        }
    }
}

async fn try_matchmaking(
    data: Arc<Data>,
    cache_http: Arc<Http>,
    guild_id: GuildId,
) -> Result<Option<f32>, Error> {
    let (team_count, total_player_count) = {
        let configuration = data.configuration.lock().unwrap();
        let queued_players = data.queued_players.lock().unwrap();
        let total_player_count = configuration.team_count * configuration.team_size;
        if (queued_players.len() as u32) < total_player_count {
            return Ok(None);
        }
        (configuration.team_count, total_player_count)
    };
    let config = {
        let config = data.configuration.lock().unwrap();
        config.clone()
    };
    let Some(category) = config.category else {
        return Err(Error::from("No category"));
    };
    let queued_players = data.queued_players.lock().unwrap().clone();
    println!("Trying matchmaking");
    let members =
        greedy_matchmaking(data.clone(), queued_players).await;
    let Some(members) = members else {
        println!("Could not find valid matchmaking");
        let delay = 10.0;
        return Ok(Some(delay));
    };
    let cost_eval = evaluate_cost(data.clone(), &members).await;
    if cost_eval.0 > config.maximum_queue_cost {
        println!("Best option has cost of {}", cost_eval.0);
        let delay = (cost_eval.0 - config.maximum_queue_cost) / total_player_count as f32 + 1.0;
        return Ok(Some(delay));
    }
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
    for (category_name, value) in cost_eval.1 {
        members_message += format!(
            "{}: {}\n",
            category_name,
            config.game_categories[&category_name][value].mention()
        )
        .as_str();
    }
    for (team_idx, team) in members.iter().enumerate() {
        members_message += format!("## Team {}\n", team_idx + 1).as_str();
        for player in team {
            members_message += format!("{}\n", player.mention()).as_str();
        }
    }
    match_channel
        .send_message(
            cache_http.clone(),
            CreateMessage::default()
                .allowed_mentions(
                    CreateAllowedMentions::default()
                        .all_roles(false)
                        .all_users(true),
                )
                .content(members_message),
        )
        .await?;
    if config.map_vote_count > 0 {
        let mut map_vote_message_content = "# Map Vote".to_string();
        if config.map_vote_time > 0 {
            map_vote_message_content += format!(
                "\nEnds <t:{}:R>",
                std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() + config.map_vote_time as u64
            )
            .as_str();
        }
        let mut map_vote_message = CreateMessage::default().content(map_vote_message_content);
        let mut map_pool = config.maps.clone();
        let mut maps = vec![];
        for _ in 0..config.map_vote_count {
            let num = rand::thread_rng().gen_range(0..map_pool.len());
            let rand_map = map_pool.remove(num);
            maps.push(rand_map.clone());
            map_vote_message = map_vote_message.button(
                CreateButton::new(format!("map_{}", rand_map).clone())
                    .label(rand_map)
                    .style(serenity::ButtonStyle::Secondary),
            );
        }
        let mut map_message = match_channel
            .send_message(cache_http.clone(), map_vote_message)
            .await?;
        if config.map_vote_time > 0 {
            let ctx1 = Arc::clone(&cache_http);
            let data = data.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(config.map_vote_time as u64)).await;
                if map_message.components.is_empty() {
                    return;
                }
                let vote_result = {
                    let match_data = data.match_data.lock().unwrap();
                    let match_number = new_idx;
                    let mut votes: HashMap<String, u32> = HashMap::new();
                    let Some(match_data) = match_data.get(&match_number) else {
                        return;
                    };
                    for (_user, vote) in match_data.map_votes.iter() {
                        let current_votes = votes.get(vote).unwrap_or(&0);
                        votes.insert(vote.clone(), current_votes + 1);
                    }
                    votes
                        .iter()
                        .max_by_key(|(_category, vote_count)| *vote_count)
                        .map(|(category, _vote_count)| category.clone())
                        .unwrap_or(maps[0].clone())
                        .clone()
                };

                map_message
                    .edit(ctx1.clone(), EditMessage::new().components(vec![]))
                    .await
                    .ok();
                let content = format!("# Map: {}", vote_result);

                map_message
                    .edit(ctx1.clone(), EditMessage::new().content(content))
                    .await
                    .ok();
            });
        }
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
                .label(format!("Team {}", i + 1))
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
                    .member(cache_http.clone(), player)
                    .await?
                    .edit(
                        cache_http.clone(),
                        EditMember::new().voice_channel(vc_channels.get(team_idx).unwrap().clone()),
                    )
                    .await
                    .ok();
                data.queued_players.lock().unwrap().remove(player);
                data.in_game_players.lock().unwrap().insert(player.clone());
                data.player_data
                    .lock()
                    .unwrap()
                    .get_mut(player)
                    .unwrap()
                    .queue_enter_time = None;
            }
        }
    }
    Ok(None)
}

async fn evaluate_cost(
    data: Arc<Data>,
    players: &Vec<Vec<UserId>>,
) -> (f32, HashMap<String, usize>) {
    let player_game_data = {
        let player_data = data.player_data.lock().unwrap();
        players
            .iter()
            .map(|team| {
                team.iter()
                    .map(|player| player_data.get(player).unwrap().clone())
                    .collect_vec()
            })
            .collect_vec()
    };
    let (team_size, game_categories, default_player_data) = {
        let config = data.configuration.lock().unwrap();
        (
            config.team_size,
            config.game_categories.clone(),
            config.default_player_data.clone(),
        )
    };
    let team_mmrs = player_game_data.iter().map(|team| {
        team.iter()
            .map(|player| player.rating.unwrap_or(default_player_data.rating).rating as f32)
            .sum::<f32>()
            / team_size as f32
    });
    let mmr_differential = match team_mmrs.minmax() {
        MinMaxResult::NoElements => 0.0,
        MinMaxResult::OneElement(_) => 0.0,
        MinMaxResult::MinMax(min, max) => max - min,
    };
    let mmr_range = player_game_data
        .iter()
        .flat_map(|team| {
            team.iter()
                .map(|player| player.rating.unwrap_or(default_player_data.rating).rating as f32)
        })
        .minmax();
    let mmr_range = match mmr_range {
        MinMaxResult::NoElements => 0.0,
        MinMaxResult::OneElement(_) => 0.0,
        MinMaxResult::MinMax(min, max) => max - min,
    };

    let player_categories: Vec<HashMap<String, Vec<usize>>> = player_game_data
        .iter()
        .flat_map(|team| team.iter().map(|player| player.game_categories.clone()))
        .collect_vec();
    let game_categories: HashMap<String, usize> = game_categories
        .iter()
        .map(|(category_name, roles)| {
            let players_category_values = player_categories
                .iter()
                .map(|player_categories| player_categories[category_name].clone())
                .collect_vec();
            let mut counts = vec![0; roles.len()];
            for player_category_values in players_category_values {
                for category_value in player_category_values {
                    counts[category_value] += 1;
                }
            }
            (
                category_name.clone(),
                if let Some((category, _count)) = counts
                    .iter()
                    .enumerate()
                    .max_by_key(|&(_category, count)| count)
                {
                    category
                } else {
                    0
                },
            )
        })
        .collect();
    let now = chrono::offset::Utc::now();
    (
        player_game_data
            .iter()
            .flat_map(|team| team.iter())
            .zip(player_categories.iter())
            .map(|(player, player_categories)| {
                let queue_config = player
                    .player_queueing_config
                    .derive(&default_player_data.player_queueing_config);
                let time_in_queue = (now - player.queue_enter_time.unwrap()).num_seconds();
                let mut player_cost = 0.0;
                player_cost += (mmr_differential - queue_config.acceptable_mmr_differential)
                    .max(0.0)
                    * queue_config.cost_per_avg_mmr_differential;
                player_cost += (mmr_range - queue_config.acceptable_mmr_range).max(0.0)
                    * queue_config.cost_per_mmr_range;
                player_cost += queue_config
                    .wrong_game_category_cost
                    .iter()
                    .filter(|(category, _)| {
                        !player_categories[*category].contains(&game_categories[*category])
                    })
                    .map(|(_, cost)| cost)
                    .sum::<f32>();
                player_cost -= time_in_queue as f32;
                player_cost
            })
            .sum(),
        game_categories,
    )
}

async fn greedy_matchmaking(
    data: Arc<Data>,
    pool: HashSet<UserId>,
) -> Option<Vec<Vec<UserId>>> {
    let team_size = data.configuration.lock().unwrap().team_size;
    let team_count = data.configuration.lock().unwrap().team_count;
    let total_players = team_size * team_count;
    let mut players = pool.clone();
    let mut result = vec![vec![]; team_count as usize];
    let mut player_count = 0;

    while player_count < total_players {
        println!("Player count: {}", player_count);
        let mut min_cost = f32::MAX;
        let mut best_next_result = vec![];
        let mut best_added_players = vec![];
        for possible_addition in players.iter() {
            for team_idx in 0..team_count as usize {
                if result[team_idx].len() >= team_size as usize {
                    continue;
                }
                let mut result_copy = result.clone();
                let mut added_players = vec![];
                if let Some(party) = data
                    .player_data
                    .lock()
                    .unwrap()
                    .get(possible_addition)
                    .unwrap()
                    .party
                {
                    for player in data
                        .group_data
                        .lock()
                        .unwrap()
                        .get(&party)
                        .unwrap()
                        .players
                        .iter()
                    {
                        added_players.push(player.clone());
                        result_copy[team_idx].push(player.clone());
                    }
                } else {
                    added_players.push(possible_addition.clone());
                    result_copy[team_idx].push(possible_addition.clone());
                }

                let cost = evaluate_cost(data.clone(), &result_copy)
                    .await
                    .0;
                if cost < min_cost {
                    min_cost = cost;
                    best_next_result = result_copy;
                    best_added_players = added_players;
                }
            }
        }

        if min_cost == f32::MAX {
            return None;
        }
        result = best_next_result;
        player_count += best_added_players.len() as u32;
        for added_player in best_added_players {
            players.remove(&added_player);
        }
    }

    Some(result)
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

/// Displays or sets number of maps for the vote
#[poise::command(slash_command, prefix_command, rename = "map_vote_time")]
async fn configure_map_vote_time(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<u32>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.map_vote_time = new_value;
        }
        let response = format!("Map vote time set to {}", new_value);
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!("Map vote time is currently {}", data_lock.map_vote_time)
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Displays or sets number of maps for the vote
#[poise::command(slash_command, prefix_command, rename = "maximum_queue_cost")]
async fn configure_maximum_queue_cost(
    ctx: Context<'_>,
    #[description = "New value"] new_value: Option<f32>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.maximum_queue_cost = new_value;
        }
        let response = format!("Max queue cost set to {}", new_value);
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!(
                "Max queue cost is currently {}",
                data_lock.maximum_queue_cost
            )
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
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn backup(ctx: Context<'_>) -> Result<(), Error> {
    {
        let match_idx = ctx.data().queue_idx.lock().unwrap().clone();
        let config = serde_json::to_string_pretty(ctx.data())?;
        println!("Starting backup...");
        fs::write(format!("backups/backup_{}.json", match_idx), config)?;
        println!("Backup made!");
    }
    let response = format!("Backup made.");
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Exports configuration
#[poise::command(slash_command, prefix_command)]
async fn export_config(ctx: Context<'_>) -> Result<(), Error> {
    let config = serde_json::to_string_pretty(&ctx.data().configuration.lock().unwrap().clone())?;
    let response = format!("Configuration: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Imports configuration
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn import_config(
    ctx: Context<'_>,
    #[description = "New config"] new_config: String,
) -> Result<(), Error> {
    let new_config: QueueConfiguration = serde_json::from_str(&new_config.as_str())?;
    *ctx.data().configuration.lock().unwrap() = new_config;
    let config = serde_json::to_string_pretty(ctx.data())?;
    let response = format!("Configuration set to: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Join queue
#[poise::command(slash_command, prefix_command)]
async fn queue(ctx: Context<'_>) -> Result<(), Error> {
    match try_queue_player(
        ctx.data().clone(),
        ctx.author().id,
        ctx.http(),
        ctx.guild_id().unwrap(),
    )
    .await
    {
        Ok(()) => {
            let response = {
                let data_lock = ctx.data().queued_players.lock().unwrap();
                format!(
                    "Queued players: {}",
                    data_lock.iter().map(|c| c.mention()).join(", ")
                )
            };
            ctx.send(CreateReply::default().content(response).ephemeral(true))
                .await?;
            try_matchmaking(
                ctx.data().clone(),
                ctx.serenity_context().http.clone(),
                ctx.guild_id().unwrap(),
            )
            .await?;
            Ok(())
        }
        Err(reason) => {
            ctx.send(CreateReply::default().content(reason).ephemeral(true))
                .await?;
            Ok(())
        }
    }
}

/// Join queue
#[poise::command(slash_command, prefix_command)]
async fn leave_queue(ctx: Context<'_>) -> Result<(), Error> {
    let removed = {
        let mut queued_players = ctx.data().queued_players.lock().unwrap();
        let mut player_data = ctx.data().player_data.lock().unwrap();
        player_data
            .get_mut(&ctx.author().id)
            .unwrap()
            .queue_enter_time = None;
        queued_players.remove(&ctx.author().id)
    };
    if removed {
        ctx.send(
            CreateReply::default()
                .content("You are no longer queueing!")
                .ephemeral(true),
        )
        .await?;
        Ok(())
    } else {
        ctx.send(
            CreateReply::default()
                .content("You weren't queued!")
                .ephemeral(true),
        )
        .await?;
        Ok(())
    }
}

/// Lists queued players
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

/// Lists parties
#[poise::command(slash_command, prefix_command)]
async fn list_parties(ctx: Context<'_>) -> Result<(), Error> {
    let response = {
        let groups = ctx.data().group_data.lock().unwrap().clone();
        format!("Groups: {}", serde_json::to_string(&groups).unwrap())
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
        "configure_map_vote_count",
        "configure_map_vote_time",
        "configure_maximum_queue_cost"
    )
)]
async fn configure(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Shows player stats
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
            .or_insert(DerivedPlayerData::default())
            .rating
            .unwrap_or(config.default_player_data.rating)
    };
    let response = format!(
        "{}'s mmr is {}, with uncertainty {}",
        user.mention(),
        rating.rating,
        rating.uncertainty
    );
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

#[poise::command(prefix_command, required_permissions = "MANAGE_CHANNELS")]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

/// Invites player to party
#[poise::command(slash_command, prefix_command, rename = "invite")]
async fn party_invite(
    ctx: Context<'_>,
    #[description = "Invite player to party"] user: UserId,
) -> Result<(), Error> {
    let party = {
        let mut user_data = ctx.data().player_data.lock().unwrap();
        let user_data = user_data
            .entry(ctx.author().id)
            .or_insert(DerivedPlayerData::default());
        if user_data.party.is_none() {
            user_data.party = Some(Uuid::new_v4());
        }
        user_data.party.unwrap()
    };
    let user_party = ctx
        .data()
        .group_data
        .lock()
        .unwrap()
        .entry(party)
        .or_insert(QueueGroup {
            players: HashSet::from([ctx.author().id]),
        })
        .clone();
    user.create_dm_channel(ctx)
        .await?
        .send_message(
            ctx,
            CreateMessage::default()
                .content(format!(
                    "{} invited you to their group.\nCurrent members: {}",
                    ctx.author().mention(),
                    user_party
                        .players
                        .iter()
                        .map(|p| format!("{}", p.mention()))
                        .join(", ")
                ))
                .button(
                    CreateButton::new(format!(
                        "join_party_{}",
                        serde_json::to_string(&party).unwrap()
                    ))
                    .label("Join party")
                    .style(serenity::ButtonStyle::Success),
                )
                .button(
                    CreateButton::new(format!(
                        "reject_party_{}",
                        serde_json::to_string(&party).unwrap()
                    ))
                    .label("Reject invite")
                    .style(serenity::ButtonStyle::Danger),
                ),
        )
        .await?;
    ctx.send(
        CreateReply::default()
            .content(format!("Invited {} to your party", user.mention()))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

async fn leave_party(
    data: Arc<Data>,
    user: &UserId,
    http: Arc<impl CacheHttp>,
    old_party: Uuid,
) -> Result<(), Error> {
    let remaining_party_members = {
        let mut group_data = data.group_data.lock().unwrap();
        let user_party = group_data.get_mut(&old_party).unwrap();
        user_party.players.remove(user);
        if user_party.players.len() == 0 {
            group_data.remove(&old_party);
            HashSet::new()
        } else {
            user_party.players.clone()
        }
    };
    for remaining_party_member in remaining_party_members {
        remaining_party_member
            .direct_message(
                http.clone(),
                CreateMessage::new().content(format!("{} left your group", user.mention())),
            )
            .await?;
    }
    Ok(())
}

/// Leave party
#[poise::command(slash_command, prefix_command, rename = "leave")]
async fn party_leave(ctx: Context<'_>) -> Result<(), Error> {
    let old_party = {
        let mut user_data = ctx.data().player_data.lock().unwrap();
        let user_data = user_data
            .entry(ctx.author().id)
            .or_insert(DerivedPlayerData::default());
        let old_party = user_data.party.clone();
        user_data.party = None;
        old_party
    };
    let Some(old_party) = old_party else {
        ctx.send(
            CreateReply::default()
                .content(format!("You weren't in a party"))
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    leave_party(
        ctx.data().clone(),
        &ctx.author().id,
        Arc::new(ctx.http()),
        old_party,
    )
    .await?;
    ctx.send(
        CreateReply::default()
            .content(format!("Left party"))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// List party members
#[poise::command(slash_command, prefix_command, rename = "list")]
async fn party_list(ctx: Context<'_>) -> Result<(), Error> {
    let party = {
        let mut user_data = ctx.data().player_data.lock().unwrap();
        let user_data = user_data
            .entry(ctx.author().id)
            .or_insert(DerivedPlayerData::default());
        user_data.party.clone()
    };
    let Some(party) = party else {
        ctx.send(
            CreateReply::default()
                .content(format!("You aren't in a party"))
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    let party_members = {
        let mut group_data = ctx.data().group_data.lock().unwrap();
        let user_party = group_data.get_mut(&party).unwrap();
        user_party.players.clone()
    };
    ctx.send(
        CreateReply::default()
            .content(format!(
                "Party members: {}",
                party_members.iter().map(|p| p.mention()).join(", ")
            ))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Displays your or another user's account creation date
#[poise::command(
    slash_command,
    prefix_command,
    subcommands("party_invite", "party_leave", "party_list")
)]
async fn party(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Displays a leaderboard
#[poise::command(slash_command, prefix_command)]
async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    let default_rating = ctx
        .data()
        .configuration
        .lock()
        .unwrap()
        .default_player_data
        .rating;
    let mut player_data = ctx
        .data()
        .player_data
        .lock()
        .unwrap()
        .iter()
        .map(|(id, data)| (id.mention(), data.rating.unwrap_or(default_rating).rating))
        .collect_vec();
    player_data.sort_by(|(_, rating_a), (_, rating_b)| rating_b.partial_cmp(rating_a).unwrap());
    let mut response = "## Leaderboard\n".to_string();
    for (idx, (player, rating)) in player_data.iter().enumerate().take(10) {
        response += format!("#{} {}: {}\n", idx + 1, player, rating).as_str();
    }
    ctx.send(
        CreateReply::default()
            .content(response)
            .ephemeral(true)
            .allowed_mentions(CreateAllowedMentions::new().all_users(false)),
    )
    .await?;
    Ok(())
}

fn update_bans(data: Arc<Data>) {
    let now = chrono::offset::Utc::now();
    data.player_bans.lock().unwrap().retain(
        |_,
         BanData {
             end_time,
             reason: _,
         }| {
            if let Some(end_time) = end_time {
                *end_time > now
            } else {
                true
            }
        },
    )
}

/// Bans a player from queueing
#[poise::command(slash_command, prefix_command, rename = "ban")]
async fn ban_player(
    ctx: Context<'_>,
    #[description = "Player"] player: UserId,
    #[description = "Reason"] reason: Option<String>,
    #[description = "Days"] days: Option<u32>,
) -> Result<(), Error> {
    update_bans(ctx.data().clone());
    let end_time = days.map(|days| {
        chrono::offset::Utc::now() + TimeDelta::new(60 * 60 * 24 * days as i64, 0).unwrap()
    });
    let ban_data: BanData = BanData { end_time, reason };
    let was_previously_banned = ctx
        .data()
        .player_bans
        .lock()
        .unwrap()
        .insert(player, ban_data)
        .is_some();

    let response = if was_previously_banned {
        format!("{}'s ban was updated.", player.mention())
    } else {
        format!("{} banned", player.mention())
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Unbans a player from queueing
#[poise::command(slash_command, prefix_command, rename = "unban")]
async fn unban_player(
    ctx: Context<'_>,
    #[description = "Player"] player: UserId,
) -> Result<(), Error> {
    update_bans(ctx.data().clone());
    let was_banned = ctx
        .data()
        .player_bans
        .lock()
        .unwrap()
        .remove(&player)
        .is_some();

    let response = if was_banned {
        format!("Unbanned {}.", player.mention())
    } else {
        format!("{} was not banned.", player.mention())
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Lists players banned from queueing
#[poise::command(slash_command, prefix_command)]
async fn list_bans(ctx: Context<'_>) -> Result<(), Error> {
    update_bans(ctx.data().clone());
    let ban_data = ctx
        .data()
        .player_bans
        .lock()
        .unwrap()
        .iter()
        .map(|(id, ban_data)| {
            let mut ban = format!("{} banned", id.mention());
            if let Some(reason) = ban_data.reason.clone() {
                ban += format!(" for {}", reason).as_str();
            }
            if let Some(end_time) = ban_data.end_time {
                ban += format!(" until <t:{}:f>", end_time.timestamp()).as_str();
            }
            ban
        })
        .join("\n");

    let response = format!("# Player Bans\n{}", ban_data);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Gets player info
#[poise::command(slash_command, prefix_command)]
async fn get_player(
    ctx: Context<'_>,
    #[description = "Player"] player: UserId,
) -> Result<(), Error> {
    let player_data = ctx
        .data()
        .player_data
        .lock()
        .unwrap()
        .get(&player)
        .unwrap_or(&DerivedPlayerData::default())
        .clone();

    let response = format!(
        "{}'s data```json\n{}\n```",
        player.mention(),
        serde_json::to_string_pretty(&player_data).unwrap()
    );
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Manage a user
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "BAN_MEMBERS",
    subcommands("ban_player", "unban_player", "list_bans", "get_player")
)]
async fn manage_player(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Marks a player as leaver
#[poise::command(slash_command, prefix_command)]
async fn mark_leaver(
    ctx: Context<'_>,
    #[description = "Player"] player: UserId,
) -> Result<(), Error> {
    let match_number = {
        let match_channels = ctx.data().match_channels.lock().unwrap();
        match_channels.get(&ctx.channel_id()).cloned()
    };
    let Some(match_number) = match_number else {
        ctx.send(CreateReply::default().content("This command must be done in a match channel!").ephemeral(true))
            .await?;
        return Ok(())
    };
    if !ctx.data().match_data.lock().unwrap().get(&match_number).unwrap().members.iter().flatten().contains(&player) {
        ctx.send(CreateReply::default().content("This player is not in this match!").ephemeral(true))
            .await?;
        return Ok(())
    }
    let mut leaver_message_content = format!("# Did you leave {}?", player.mention());
    leaver_message_content += format!(
        "\nEnds <t:{}:R>, otherwise user will be reported",
        std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() + ctx.data().configuration.lock().unwrap().leaver_verification_time as u64
    )
    .as_str();
    let mut leaver_message = CreateReply::default().content(leaver_message_content);
    leaver_message = leaver_message.components(vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("leaver_check_{}", player.get()).clone())
            .label("No, I'm here.")
            .style(serenity::ButtonStyle::Primary),
    ])]);
    let leaver_message = ctx.send(leaver_message).await?.message().await?.id;
    {
        let data = ctx.data().clone();
        let guild_id = ctx.guild_id().unwrap();
        let channel_id = ctx.channel_id();
        let ctx1 = ctx.serenity_context().http.clone();
        tokio::spawn(async move {
            let leaver_verification_time = data.clone().configuration.lock().unwrap().leaver_verification_time as u64;
            tokio::time::sleep(Duration::from_secs(leaver_verification_time)).await;
            let Ok(message) = ctx1.get_message(channel_id, leaver_message).await else {
                return;
            };
            message.delete(ctx1.clone()).await.ok();
            let Ok(mut member) = guild_id.member(ctx1.clone(), player).await else {
                return;
            };
            member.edit(
                    ctx1,
                    EditMember::new().disconnect_member(),
                )
                .await
                .ok();
            *data.leaver_data.lock().unwrap().entry(player).or_insert(0) += 1;
        });
    }
    
    Ok(())
}

/// Lists players who've left games
#[poise::command(slash_command, prefix_command)]
async fn list_leavers(ctx: Context<'_>) -> Result<(), Error> {
    let leave_data = ctx
        .data()
        .leaver_data
        .lock()
        .unwrap()
        .iter()
        .map(|(id, count)| {
            format!("{} left {} times", id.mention(), count)
        })
        .join("\n");

    let response = format!("# Player Leave Counts\n{}", leave_data);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}
#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_BOT_TOKEN").expect("missing DISCORD_BOT_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            event_handler: |ctx, event, framework, data| {
                Box::pin(handler(ctx, event, framework, data.clone()))
            },
            commands: vec![
                register(),
                configure(),
                backup(),
                export_config(),
                import_config(),
                queue(),
                leave_queue(),
                list_queued(),
                stats(),
                party(),
                list_parties(),
                leaderboard(),
                manage_player(),
                mark_leaver(),
                list_leavers(),
            ],
            ..Default::default()
        })
        .setup(|_ctx, _ready, _framework| {
            Box::pin(async move {
                let config_data: Option<Arc<Data>> =
                    fs::read_to_string("config.json").ok().map(|read| {
                        serde_json::from_str(read.as_str()).expect("Failed to parse config file")
                    });
                if let Some(data) = config_data {
                    return Ok(data);
                }
                Ok(Arc::new(Data::default()))
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}
