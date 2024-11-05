mod admin_commands;
mod configure_command;
mod player_config_commands;

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    fs::{self, OpenOptions},
    hash::Hash,
    io::prelude::*,
    sync::{Arc, Mutex},
    time::Duration,
};

use admin_commands::{
    create_queue_message, create_register_message, create_roles_message, force_outcome,
    list_leavers, manage_player, register,
};
use chrono::{DateTime, Utc};
use configure_command::{configure, create_queue, export_config, import_config};
use dashmap::DashMap;
use hopcroft_karp::matching;
use itertools::{Itertools, MinMaxResult};
use player_config_commands::player_config;
use poise::{
    serenity_prelude::{
        self as serenity, futures::future, Builder, CacheHttp, ChannelId, ChannelType,
        ComponentInteraction, ComponentInteractionDataKind, CreateActionRow, CreateAllowedMentions,
        CreateButton, CreateChannel, CreateInteractionResponse, CreateInteractionResponseMessage,
        CreateMessage, EditInteractionResponse, EditMember, EditMessage, GuildId, Http,
        Mentionable, MessageId, PermissionOverwrite, PermissionOverwriteType, Permissions, RoleId,
        UserId, VoiceState,
    },
    CreateReply,
};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use skillratings::{
    weng_lin::{WengLin, WengLinConfig, WengLinRating},
    MultiTeamOutcome, MultiTeamRatingSystem,
};
use tokio::sync::Notify;

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, Hash, Copy)]
struct MatchUuid(uuid::Uuid);

impl Display for MatchUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl MatchUuid {
    fn new() -> Self {
        MatchUuid(uuid::Uuid::new_v4())
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, Hash, Copy)]
struct GroupUuid(uuid::Uuid);

impl Display for GroupUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl GroupUuid {
    fn new() -> Self {
        GroupUuid(uuid::Uuid::new_v4())
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, Hash, Copy)]
struct QueueUuid(uuid::Uuid);

impl QueueUuid {
    fn new() -> Self {
        QueueUuid(uuid::Uuid::new_v4())
    }
}

#[derive(Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    global_player_data: Mutex<HashMap<UserId, GlobalPlayerData>>,
    #[serde(default)]
    match_channels: Mutex<HashMap<ChannelId, MatchUuid>>,
    #[serde(default)]
    match_data: Mutex<HashMap<MatchUuid, MatchData>>,
    #[serde(default)]
    historical_match_data: Mutex<HashMap<MatchUuid, MatchData>>,
    #[serde(default)]
    group_data: Mutex<HashMap<GroupUuid, QueueGroup>>,
    #[serde(default)]
    guild_data: Mutex<HashMap<GuildId, GuildData>>,
    #[serde(default)]
    configuration: DashMap<QueueUuid, QueueConfiguration>,
    #[serde(default)]
    queued_players: DashMap<QueueUuid, HashSet<UserId>>,
    #[serde(default)]
    current_games: DashMap<QueueUuid, HashSet<MatchUuid>>,
    #[serde(skip)]
    message_edit_notify: DashMap<QueueUuid, Arc<Notify>>,
    #[serde(default)]
    queue_idx: DashMap<QueueUuid, u32>,
    #[serde(default)]
    player_bans: DashMap<QueueUuid, HashMap<UserId, BanData>>,
    #[serde(default)]
    leaver_data: DashMap<QueueUuid, HashMap<UserId, u32>>,
    #[serde(default)]
    player_data: DashMap<QueueUuid, HashMap<UserId, DerivedPlayerData>>,
    #[serde(default)]
    is_matchmaking: DashMap<QueueUuid, Option<()>>,
} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Arc<Data>, Error>;

impl Default for Data {
    fn default() -> Self {
        Self {
            global_player_data: Mutex::new(HashMap::new()),
            match_channels: Mutex::new(HashMap::new()),
            match_data: Mutex::new(HashMap::new()),
            historical_match_data: Mutex::new(HashMap::new()),
            group_data: Mutex::new(HashMap::new()),
            guild_data: Mutex::new(HashMap::new()),
            configuration: DashMap::new(),
            queue_idx: DashMap::new(),
            queued_players: DashMap::new(),
            current_games: DashMap::new(),
            player_data: DashMap::new(),
            player_bans: DashMap::new(),
            leaver_data: DashMap::new(),
            message_edit_notify: DashMap::new(),
            is_matchmaking: DashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct BanData {
    end_time: Option<DateTime<Utc>>,
    reason: Option<String>,
    shadow_ban: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct GuildData {
    queues: Vec<QueueUuid>,
}

impl Default for GuildData {
    fn default() -> Self {
        Self {
            queues: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct QueueGroup {
    players: HashSet<UserId>,
    pending_invites: HashSet<UserId>,
}

enum VoteType {
    None,
    Map,
    Result,
}

#[derive(Serialize, Deserialize, Clone)]
struct RoleConfiguration {
    name: String,
    description: String,
}

#[derive(Serialize, Deserialize, Clone)]
enum QueueMessageType {
    Queue,
    Roles,
    Register,
}

#[derive(Serialize, Deserialize, Clone)]
struct QueueConfiguration {
    team_size: u32,
    team_count: u32,
    category: Option<ChannelId>,
    queue_channels: HashSet<ChannelId>,
    visability_override_roles: HashSet<RoleId>,
    post_match_channel: Option<ChannelId>,
    queue_messages: Vec<(ChannelId, MessageId, QueueMessageType)>,
    register_role: Option<RoleId>,
    audit_channel: Option<ChannelId>,
    maps: HashSet<String>,
    map_vote_count: u32,
    map_vote_time: u32,
    prevent_recent_maps: bool,
    leaver_verification_time: u32,
    default_player_data: PlayerData,
    maximum_queue_cost: f32,
    incorrect_roles_cost: f32,
    game_categories: HashMap<String, Vec<RoleId>>,
    roles: HashMap<String, RoleConfiguration>,
    role_combinations: Vec<(Vec<String>, f32)>,
    log_chats: bool,
    max_lobby_keep_time: u64,
}

impl Default for QueueConfiguration {
    fn default() -> Self {
        Self {
            team_size: 5,
            team_count: 2,
            category: None,
            queue_channels: HashSet::new(),
            visability_override_roles: HashSet::new(),
            post_match_channel: None,
            queue_messages: vec![],
            register_role: None,
            audit_channel: None,
            maps: HashSet::new(),
            map_vote_count: 0,
            map_vote_time: 0,
            prevent_recent_maps: false,
            leaver_verification_time: 30,
            default_player_data: PlayerData::default(),
            maximum_queue_cost: 50.0,
            incorrect_roles_cost: 10.0,
            game_categories: HashMap::new(),
            roles: HashMap::new(),
            role_combinations: vec![],
            log_chats: true,
            max_lobby_keep_time: 15 * 60,
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
                MatchResult::Team(num) => format!("Team {}", num + 1),
                MatchResult::Tie => "Tie".to_string(),
                MatchResult::Cancel => "Cancel".to_string(),
            }
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MatchData {
    result_votes: HashMap<UserId, MatchResult>,
    map_votes: HashMap<UserId, String>,
    channels: Vec<ChannelId>,
    members: Vec<Vec<UserId>>,
    host: Option<UserId>,
    map_vote_end_time: Option<u64>,
    match_end_time: Option<u64>,
    resolved: bool,
    name: String,
    queue: QueueUuid,
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerQueueingConfig {
    cost_per_avg_mmr_differential: f32,
    acceptable_mmr_differential: f32,
    cost_per_mmr_std_differential: f32,
    acceptable_mmr_std_differential: f32,
    cost_per_mmr_range: f32,
    acceptable_mmr_range: f32,
    new_lobby_host_cost: f32,
    wrong_game_category_cost: HashMap<String, f32>,
    active_roles: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct DerivedPlayerQueueingConfig {
    cost_per_avg_mmr_differential: Option<f32>,
    acceptable_mmr_differential: Option<f32>,
    cost_per_mmr_std_differential: Option<f32>,
    acceptable_mmr_std_differential: Option<f32>,
    cost_per_mmr_range: Option<f32>,
    acceptable_mmr_range: Option<f32>,
    new_lobby_host_cost: Option<f32>,
    wrong_game_category_cost: Option<HashMap<String, f32>>,
    active_roles: Option<Vec<String>>,
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
            cost_per_mmr_std_differential: self
                .cost_per_mmr_std_differential
                .unwrap_or(base.cost_per_mmr_std_differential),
            acceptable_mmr_std_differential: self
                .acceptable_mmr_std_differential
                .unwrap_or(base.acceptable_mmr_std_differential),
            cost_per_mmr_range: self.cost_per_mmr_range.unwrap_or(base.cost_per_mmr_range),
            acceptable_mmr_range: self
                .acceptable_mmr_range
                .unwrap_or(base.acceptable_mmr_range),
            new_lobby_host_cost: self
                .new_lobby_host_cost
                .unwrap_or(base.acceptable_mmr_range),
            wrong_game_category_cost: self
                .wrong_game_category_cost
                .clone()
                .unwrap_or(base.wrong_game_category_cost.clone()),
            active_roles: self
                .active_roles
                .clone()
                .unwrap_or(base.active_roles.clone()),
        }
    }
}

impl Default for DerivedPlayerQueueingConfig {
    fn default() -> DerivedPlayerQueueingConfig {
        DerivedPlayerQueueingConfig {
            cost_per_avg_mmr_differential: None,
            acceptable_mmr_differential: None,
            cost_per_mmr_std_differential: None,
            acceptable_mmr_std_differential: None,
            cost_per_mmr_range: None,
            acceptable_mmr_range: None,
            new_lobby_host_cost: None,
            wrong_game_category_cost: None,
            active_roles: None,
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
                acceptable_mmr_differential: 1.0,
                cost_per_mmr_std_differential: 0.02,
                acceptable_mmr_std_differential: 2.0,
                cost_per_mmr_range: 0.02,
                acceptable_mmr_range: 3.0,
                new_lobby_host_cost: 5.0,
                wrong_game_category_cost: HashMap::new(),
                active_roles: vec![],
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct PlayerStats {
    wins: u32,
    losses: u32,
    draws: u32,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            wins: 0,
            losses: 0,
            draws: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct DerivedPlayerData {
    rating: Option<WengLinRating>,
    player_queueing_config: DerivedPlayerQueueingConfig,
    game_categories: HashMap<String, Vec<usize>>,
    stats: PlayerStats,
    game_history: Vec<MatchUuid>,
}

impl Default for DerivedPlayerData {
    fn default() -> Self {
        Self {
            rating: None,
            player_queueing_config: DerivedPlayerQueueingConfig::default(),
            game_categories: HashMap::new(),
            stats: PlayerStats::default(),
            game_history: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
enum QueueState {
    None,
    Queued,
    InGame,
}

#[derive(Serialize, Deserialize, Clone)]
struct GlobalPlayerData {
    queue_enter_time: Option<DateTime<Utc>>,
    party: Option<GroupUuid>,
    queue_state: QueueState,
}

impl Default for GlobalPlayerData {
    fn default() -> Self {
        Self {
            queue_enter_time: None,
            party: None,
            queue_state: QueueState::None,
        }
    }
}
async fn on_error(error: poise::FrameworkError<'_, Arc<Data>, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx, .. } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        poise::FrameworkError::EventHandler { error, event, .. } => {
            println!("Error in event `{}`: {:?}", event.snake_case_name(), error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {}", e)
            }
        }
    }
}

async fn try_queue_player(
    data: Arc<Data>,
    queue_id: &QueueUuid,
    user_id: UserId,
    http: Arc<Http>,
    guild_id: GuildId,
    queue_party: bool,
    is_bot: bool,
) -> Result<(), String> {
    {
        let mut player_data = data.player_data.get_mut(&queue_id).unwrap();
        player_data.entry(user_id).or_default();
    }
    if matches!(
        data.global_player_data
            .lock()
            .unwrap()
            .entry(user_id)
            .or_default()
            .queue_state,
        QueueState::InGame
    ) {
        return Err("Cannot queue while in game!".to_string());
    }
    if data
        .queued_players
        .get(&queue_id)
        .unwrap()
        .contains(&user_id)
    {
        return Err("You're already in this queue!".to_string());
    }
    if let Some(group) = data
        .global_player_data
        .lock()
        .unwrap()
        .get(&user_id)
        .unwrap()
        .party
    {
        if data
            .group_data
            .lock()
            .unwrap()
            .get(&group)
            .unwrap()
            .pending_invites
            .len()
            > 0
        {
            return Err("Cannot queue while your party has pending invites! Do `/party leave` to exit party.".to_string());
        }
    }
    for queue in data
        .guild_data
        .lock()
        .unwrap()
        .get(&guild_id)
        .unwrap()
        .queues
        .iter()
        .cloned()
    {
        update_bans(data.clone(), &queue);
    }
    let game_categories = {
        let config = data.configuration.get(&queue_id).unwrap();
        config.game_categories.clone()
    };
    let user_roles = if is_bot {
        vec![]
    } else {
        guild_id.member(http.clone(), user_id).await.unwrap().roles
    };
    let player_categories: HashMap<String, Vec<usize>> = game_categories
        .iter()
        .map(|(category_name, category_roles)| {
            (
                category_name.clone(),
                category_roles
                    .iter()
                    .enumerate()
                    .filter(|(_, role)| user_roles.contains(role))
                    .map(|(idx, _)| idx)
                    .collect_vec(),
            )
        })
        .collect();
    {
        let mut player_data = data.player_data.get_mut(&queue_id).unwrap();
        player_data.get_mut(&user_id).unwrap().game_categories = player_categories;
        if let Some(player_ban) = data.player_bans.get(&queue_id).unwrap().get(&user_id) {
            if !player_ban.shadow_ban {
                if let Some(ban_reason) = player_ban.reason.clone() {
                    return Err(format!(
                        "Cannot queue because you're banned for {}",
                        ban_reason
                    ));
                }
                return Err("Cannot queue because you're banned".to_string());
            }
        }
    }
    let queue_enter_time = chrono::offset::Utc::now();
    {
        let mut queued_players = data.queued_players.get_mut(&queue_id).unwrap();
        queued_players.insert(user_id);
    }
    let party_id = {
        let mut global_player_data = data.global_player_data.lock().unwrap();
        let global_player_data = global_player_data
            .entry(user_id)
            .or_insert(GlobalPlayerData::default());

        global_player_data.queue_enter_time = Some(queue_enter_time.clone());
        global_player_data.queue_state = QueueState::Queued;

        global_player_data.party
    };

    if queue_party {
        if let Some(party) = party_id {
            let party_members = data
                .group_data
                .lock()
                .unwrap()
                .get(&party)
                .unwrap()
                .players
                .iter()
                .filter(|id| **id != user_id)
                .cloned()
                .collect::<Vec<_>>();

            future::join_all(party_members.iter().map(|group_member_id| {
                try_queue_player(
                    data.clone(),
                    queue_id,
                    group_member_id.clone(),
                    http.clone(),
                    guild_id,
                    false,
                    is_bot,
                )
            }))
            .await
            .into_iter()
            .collect::<Result<(), String>>()?;
        }
    }
    let queue_id = queue_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs_f32(60.0 * 30.0)).await;
            match ensure_wants_queue(
                data.clone(),
                http.clone(),
                &user_id,
                &queue_id,
                queue_enter_time,
            )
            .await
            {
                Ok(true) => break,
                Ok(false) => {}
                Err(err) => {
                    eprintln!("{}", err);
                    break;
                }
            };
        }
    });

    Ok(())
}

async fn ensure_wants_queue(
    data: Arc<Data>,
    http: Arc<Http>,
    user: &UserId,
    queue_id: &QueueUuid,
    queue_enter_time: DateTime<Utc>,
) -> Result<bool, Error> {
    if !data.queued_players.get(&queue_id).unwrap().contains(user) {
        return Ok(true);
    }
    if data
        .global_player_data
        .lock()
        .unwrap()
        .get(user)
        .unwrap()
        .queue_enter_time
        .unwrap()
        != queue_enter_time
    {
        return Ok(true);
    }
    let leaver_message_content =
        format!("# Are you still wanting to queue {}?\nEnds <t:{}:R>, otherwise you will be kicked from queue", user.mention(), 
        std::time::UNIX_EPOCH.elapsed().unwrap().as_secs()
        + data
            .configuration
            .get(&queue_id)
            .unwrap()
            .leaver_verification_time as u64);
    let leaver_message = CreateMessage::default()
        .content(leaver_message_content)
        .components(vec![CreateActionRow::Buttons(vec![
            CreateButton::new(format!("queue_check"))
                .label("Yes, I'm here.")
                .style(serenity::ButtonStyle::Primary),
            CreateButton::new(format!(
                "afk_leave_queue_{}",
                serde_json::to_string(queue_id).unwrap()
            ))
            .label("No, exit queue.")
            .style(serenity::ButtonStyle::Primary),
        ])]);
    let Ok(leaver_message) = user.direct_message(http.clone(), leaver_message).await else {
        data.queued_players
            .get_mut(&queue_id)
            .unwrap()
            .remove(&user);
        data.message_edit_notify
            .get(&queue_id)
            .unwrap()
            .notify_one();
        return Ok(true);
    };
    {
        let user = user.clone();
        let data = data.clone();
        let ctx1 = http.clone();
        let queue_id = queue_id.clone();
        tokio::spawn(async move {
            let leaver_verification_time = data
                .clone()
                .configuration
                .get(&queue_id)
                .unwrap()
                .leaver_verification_time as u64;
            tokio::time::sleep(Duration::from_secs(leaver_verification_time)).await;
            let Ok(mut message) = ctx1
                .get_message(leaver_message.channel_id, leaver_message.id)
                .await
            else {
                return;
            };
            player_leave_queue(data.clone(), user, true, &queue_id);
            message
                .edit(
                    ctx1.clone(),
                    EditMessage::new()
                        .content("Removed from queue for inactivity.")
                        .components(vec![]),
                )
                .await
                .ok();
        });
    }

    Ok(false)
}

async fn handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Arc<Data>, Error>,
    data: Arc<Data>,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { .. } => {
            println!("Ready");
            let notifies = data
                .message_edit_notify
                .iter()
                .map(|p| (p.key().clone(), p.value().clone()))
                .collect_vec();
            for (queue, notify) in notifies {
                let http = ctx.http.clone();
                let data = data.clone();
                tokio::spawn(async move {
                    loop {
                        notify.notified().await;
                        update_queue_messages(data.clone(), http.clone(), &queue)
                            .await
                            .ok();
                        tokio::time::sleep(Duration::from_secs_f32(1.0)).await;
                    }
                });
            }
        }
        serenity::FullEvent::VoiceStateUpdate { old, new } => {
            if let Some(VoiceState {
                guild_id: Some(guild_id),
                channel_id: Some(channel_id),
                user_id,
                ..
            }) = old
            {
                for queue in data
                    .guild_data
                    .lock()
                    .unwrap()
                    .entry(guild_id.clone())
                    .or_default()
                    .queues
                    .clone()
                {
                    let config = data.configuration.get(&queue).unwrap().clone();
                    if config.queue_channels.contains(&channel_id) {
                        player_leave_queue(data.clone(), user_id.clone(), true, &queue);
                    }
                }
            }
            let queues = data
                .guild_data
                .lock()
                .unwrap()
                .entry(new.guild_id.unwrap())
                .or_default()
                .queues
                .clone();
            for queue in queues.iter().filter(|queue| {
                let config = data.configuration.get(&queue).unwrap();
                new.channel_id
                    .map(|channel_id| config.queue_channels.contains(&channel_id))
                    .unwrap_or(false)
            }) {
                match try_queue_player(
                    data.clone(),
                    &queue,
                    new.user_id,
                    ctx.http.clone(),
                    new.guild_id.unwrap(),
                    true,
                    false,
                )
                .await
                {
                    Ok(()) => {
                        data.message_edit_notify
                            .get_mut(&queue)
                            .unwrap()
                            .notify_one();
                        matchmake(
                            data.clone(),
                            ctx.http.clone(),
                            new.guild_id.unwrap(),
                            &queue,
                        )
                        .await?;
                    }
                    Err(reason) => {
                        new.user_id
                            .direct_message(ctx, CreateMessage::new().content(reason))
                            .await?;
                    }
                }
            }
        }
        serenity::FullEvent::InteractionCreate { interaction } => {
            if let Some(message_component) = interaction.as_message_component() {
                log_interaction(message_component);
                let match_number = {
                    let match_channels = data.match_channels.lock().unwrap();
                    match_channels.get(&message_component.channel_id).cloned()
                };
                if let Some(match_number) = match_number {
                    let (queue, required_votes, is_user_in_match) = {
                        let match_data = data.match_data.lock().unwrap();
                        let Some(match_data) = match_data.get(&match_number) else {
                            return Ok(());
                        };
                        let config = data.configuration.get(&match_data.queue).unwrap();
                        (
                            match_data.queue,
                            config.team_count * config.team_size / 2 + 1,
                            match_data
                                .members
                                .iter()
                                .flatten()
                                .contains(&message_component.user.id),
                        )
                    };
                    let vote_type = 'vote_type: {
                        if !is_user_in_match {
                            message_component
                                .create_response(
                                    ctx,
                                    serenity::CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(format!(
                                                "You cannot vote in a game you're not in."
                                            ))
                                            .ephemeral(true),
                                    ),
                                )
                                .await?;
                            return Ok(());
                        }
                        if message_component
                            .data
                            .custom_id
                            .eq_ignore_ascii_case("volunteer_host")
                        {
                            let already_hosted = 'host_button_block: {
                                let mut match_data = data.match_data.lock().unwrap();
                                let match_data = match_data.get_mut(&match_number).unwrap();
                                if match_data.host.is_some() {
                                    break 'host_button_block true;
                                }
                                match_data.host = Some(message_component.user.id);
                                false
                            };
                            if already_hosted {
                                message_component
                                    .create_response(
                                        ctx,
                                        serenity::CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content(format!(
                                                    "There is already a host for this lobby."
                                                ))
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await?;
                                return Ok(());
                            }

                            let current_content = format!(
                                "{}\n## Host: {}",
                                message_component.message.content.clone(),
                                message_component.user.id.mention()
                            );
                            ctx.http
                                .clone()
                                .get_message(
                                    message_component.channel_id,
                                    message_component.message.id,
                                )
                                .await?
                                .edit(
                                    ctx,
                                    EditMessage::new()
                                        .components(vec![])
                                        .content(current_content),
                                )
                                .await?;
                            return Ok(());
                        }
                        let mut match_data = data.match_data.lock().unwrap();
                        let Some(match_data) = match_data.get_mut(&match_number) else {
                            eprintln!("Could not find match data for vote!");
                            break 'vote_type VoteType::None;
                        };
                        if let Some(map) = message_component.data.custom_id.strip_prefix("map_") {
                            match_data
                                .map_votes
                                .insert(message_component.user.id, map.to_string());
                            break 'vote_type VoteType::Map;
                        }

                        let match_result = if let Some(team_data) =
                            message_component.data.custom_id.strip_prefix("team_")
                        {
                            Some(MatchResult::Team(team_data.parse()?))
                        } else if message_component.data.custom_id.eq_ignore_ascii_case("tie") {
                            Some(MatchResult::Tie)
                        } else if message_component
                            .data
                            .custom_id
                            .eq_ignore_ascii_case("cancel")
                        {
                            Some(MatchResult::Cancel)
                        } else {
                            None
                        };
                        if let Some(match_result) = match_result {
                            match_data
                                .result_votes
                                .insert(message_component.user.id, match_result);
                            break 'vote_type VoteType::Result;
                        }
                        VoteType::None
                    };

                    match vote_type {
                        VoteType::Map => {
                            let (vote_result, mut content) = {
                                let match_data = data.match_data.lock().unwrap();
                                let match_data = match_data.get(&match_number).unwrap();

                                let votes = match_data
                                    .map_votes
                                    .iter()
                                    .map(|(_, vote)| vote)
                                    .counts()
                                    .into_iter()
                                    .sorted_by_key(|(_, count)| *count)
                                    .rev()
                                    .collect_vec();
                                let content = format!(
                                    "# Map Vote{}{}",
                                    match_data
                                        .map_vote_end_time
                                        .map(|map_vote_end_time| format!(
                                            "\nEnds <t:{}:R>",
                                            map_vote_end_time
                                        ))
                                        .unwrap_or("".to_string()),
                                    votes
                                        .iter()
                                        .map(|(vote_type, count)| format!(
                                            "\n{}: {}",
                                            vote_type, count
                                        ))
                                        .join("")
                                );
                                (
                                    votes
                                        .into_iter()
                                        .next()
                                        .filter(|(_, count)| *count >= required_votes as usize)
                                        .map(|(vote_type, _)| vote_type.clone()),
                                    content,
                                )
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
                                .get_message(
                                    message_component.channel_id,
                                    message_component.message.id,
                                )
                                .await?
                                .edit(ctx.http.clone(), EditMessage::new().content(content))
                                .await?;
                        }
                        VoteType::Result => {
                            if {
                                let match_data = data.match_data.lock().unwrap();
                                let match_data = match_data.get(&match_number).unwrap();
                                match_data.resolved
                            } {
                                return Ok(());
                            }
                            let (vote_result, content) = {
                                let match_data = data.match_data.lock().unwrap();
                                let votes = match_data
                                    .get(&match_number)
                                    .unwrap()
                                    .result_votes
                                    .iter()
                                    .map(|(_, vote)| vote)
                                    .counts()
                                    .into_iter()
                                    .sorted_by_key(|(_, count)| *count)
                                    .rev()
                                    .collect_vec();
                                let content = votes
                                    .iter()
                                    .map(|(vote_type, count)| format!("{}: {}\n", vote_type, count))
                                    .join("");
                                (
                                    votes
                                        .into_iter()
                                        .next()
                                        .filter(|(_, count)| *count >= required_votes as usize)
                                        .map(|(vote_type, _)| vote_type.clone()),
                                    content,
                                )
                            };
                            let Some(vote_result) = vote_result else {
                                ctx.http
                                    .clone()
                                    .get_message(
                                        message_component.channel_id,
                                        message_component.message.id,
                                    )
                                    .await?
                                    .edit(ctx.http.clone(), EditMessage::new().content(content))
                                    .await?;
                                return Ok(());
                            };
                            let post_match_channel = data
                                .configuration
                                .get(&queue)
                                .unwrap()
                                .post_match_channel
                                .clone();
                            let (channels, players) = {
                                let mut match_data = data.match_data.lock().unwrap();
                                let match_data = match_data.get_mut(&match_number).unwrap();
                                match_data.resolved = true;
                                log_match_results(data.clone(), &vote_result, &match_data);
                                (match_data.channels.clone(), match_data.members.clone())
                            };

                            apply_match_results(data.clone(), vote_result.clone(), &players, queue);

                            let guild_id = message_component.guild_id.unwrap();
                            for player in players.iter().flat_map(|t| t) {
                                data.global_player_data
                                    .lock()
                                    .unwrap()
                                    .get_mut(player)
                                    .unwrap()
                                    .queue_state = QueueState::None;
                            }
                            data.message_edit_notify
                                .get_mut(&queue)
                                .unwrap()
                                .notify_one();
                            if let Some(post_match_channel) = post_match_channel {
                                future::join_all(
                                    players
                                        .iter()
                                        .flat_map(|t| t)
                                        .filter(|player| {
                                            if let Some(Some(current_vc)) = guild_id
                                                .to_guild_cached(&ctx.cache)
                                                .unwrap()
                                                .voice_states
                                                .get(player)
                                                .map(|p| p.channel_id)
                                            {
                                                channels.contains(&current_vc)
                                            } else {
                                                false
                                            }
                                        })
                                        .map(|player| async {
                                            ctx.http
                                                .get_member(guild_id, *player)
                                                .await?
                                                .edit(
                                                    ctx.http.clone(),
                                                    EditMember::new()
                                                        .voice_channel(post_match_channel),
                                                )
                                                .await?;
                                            Ok::<(), Error>(())
                                        }),
                                )
                                .await
                                .into_iter()
                                .collect::<Result<(), _>>()
                                .ok();
                            }
                            for channel in channels {
                                data.match_channels.lock().unwrap().remove(&channel);
                                ctx.http.delete_channel(channel, None).await?;
                            }
                            {
                                let mut match_data = data.match_data.lock().unwrap();
                                let finished_match = match_data.remove(&match_number);
                                if let Some(mut finished_match) = finished_match {
                                    finished_match.match_end_time =
                                        Some(std::time::UNIX_EPOCH.elapsed().unwrap().as_secs());
                                    let mut user_data =
                                        data.player_data.get_mut(&finished_match.queue).unwrap();
                                    for user in
                                        finished_match.members.iter().flat_map(|team| team.iter())
                                    {
                                        user_data
                                            .entry(*user)
                                            .or_default()
                                            .game_history
                                            .push(match_number);
                                    }
                                    data.historical_match_data
                                        .lock()
                                        .unwrap()
                                        .insert(match_number, finished_match);
                                }
                            }
                        }
                        VoteType::None => {}
                    }
                }
                if let Some(party_id) = message_component.data.custom_id.strip_prefix("join_party_")
                {
                    if let Err(e) = {
                        let mut player_data = data.global_player_data.lock().unwrap();
                        let player_data = player_data
                            .entry(message_component.user.id)
                            .or_insert(GlobalPlayerData::default());
                        match player_data.queue_state {
                            QueueState::None => Ok(()),
                            QueueState::Queued => Err("Cannot join party while queued!"),
                            QueueState::InGame => Err("Cannot join party while in game!"),
                        }
                    } {
                        message_component
                            .create_response(
                                ctx,
                                serenity::CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("{}", e)),
                                ),
                            )
                            .await?;
                        return Ok(());
                    }
                    let party_uuid = serde_json::from_str::<GroupUuid>(party_id).unwrap();
                    let group_members = 'group_members: {
                        let mut group_data = data.group_data.lock().unwrap();
                        let party = group_data.get_mut(&party_uuid);
                        let Some(party) = party else {
                            break 'group_members Err("Party no longer exists.");
                        };
                        if !party.pending_invites.remove(&message_component.user.id) {
                            break 'group_members Err("Party invite no longer valid.");
                        }
                        party.players.insert(message_component.user.id);
                        Ok(party.players.clone())
                    };
                    let group_members = match group_members {
                        Ok(group_members) => group_members,
                        Err(e) => {
                            message_component
                                .create_response(
                                    ctx,
                                    serenity::CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(format!("{}", e)),
                                    ),
                                )
                                .await?;
                            return Ok(());
                        }
                    };
                    let old_party = {
                        let mut player_data = data.global_player_data.lock().unwrap();
                        let player_data = player_data
                            .entry(message_component.user.id)
                            .or_insert(GlobalPlayerData::default());
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
                        let mut group_data = data.group_data.lock().unwrap();
                        let party = group_data
                            .get_mut(&serde_json::from_str::<GroupUuid>(party_id).unwrap());
                        if let Some(party) = party {
                            party.pending_invites.remove(&message_component.user.id);
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
                if let Some(non_leaver_id) = message_component
                    .data
                    .custom_id
                    .strip_prefix("leaver_check_")
                {
                    let player = UserId::new(non_leaver_id.parse::<u64>().unwrap());
                    if message_component.user.id != player {
                        message_component
                            .create_response(
                                ctx,
                                serenity::CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("You aren't the right player silly :P"))
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                        return Ok(());
                    }
                    message_component.message.delete(ctx).await?;
                    message_component
                        .create_response(
                            ctx,
                            serenity::CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("You are no longer marked as a leaver."))
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    return Ok(());
                }
                if message_component.data.custom_id == "queue_check" {
                    message_component.message.delete(ctx).await?;
                    message_component
                        .create_response(
                            ctx,
                            serenity::CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("You will stay in queue."))
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    return Ok(());
                }
                if message_component.data.custom_id == "queue" {
                    let Some(queue) = get_queue(data.clone(), message_component) else {
                        message_component
                            .create_response(
                                ctx.http(),
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Could not find queue to join!")
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    message_component.defer_ephemeral(ctx.http()).await?;
                    match try_queue_player(
                        data.clone(),
                        &queue,
                        message_component.user.id,
                        ctx.http.clone(),
                        message_component.guild_id.unwrap(),
                        true,
                        false,
                    )
                    .await
                    {
                        Ok(()) => {
                            message_component
                                .edit_response(
                                    ctx.http(),
                                    EditInteractionResponse::new().content("Joined queue!"),
                                )
                                .await?;
                            data.message_edit_notify
                                .get_mut(&queue)
                                .unwrap()
                                .notify_one();
                            matchmake(
                                data.clone(),
                                ctx.http.clone(),
                                message_component.guild_id.unwrap(),
                                &queue,
                            )
                            .await?;
                        }
                        Err(reason) => {
                            message_component
                                .edit_response(
                                    ctx.http(),
                                    EditInteractionResponse::new().content(reason),
                                )
                                .await?;
                        }
                    }
                    return Ok(());
                }
                if let Some(register_mmr) =
                    message_component.data.custom_id.strip_prefix("register_")
                {
                    let Some(queue) = get_queue(data.clone(), message_component) else {
                        message_component
                            .create_response(
                                ctx.http(),
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Could not find queue to register for!")
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    message_component.defer_ephemeral(ctx.http()).await?;
                    if let Some(role) = data.configuration.get(&queue).unwrap().register_role {
                        message_component
                            .member
                            .as_ref()
                            .unwrap()
                            .add_role(ctx.http(), role)
                            .await?;
                    }
                    let default_uncertainty = data
                        .configuration
                        .get(&queue)
                        .unwrap()
                        .default_player_data
                        .rating
                        .uncertainty;
                    data.player_data
                        .get_mut(&queue)
                        .unwrap()
                        .entry(message_component.user.id)
                        .or_default()
                        .rating = Some(WengLinRating {
                        rating: register_mmr.parse::<f64>().unwrap(),
                        uncertainty: default_uncertainty,
                    });
                    message_component
                        .edit_response(
                            ctx.http(),
                            EditInteractionResponse::new().content("Registered!"),
                        )
                        .await?;
                    return Ok(());
                }
                if message_component.data.custom_id == "leave_queue" {
                    let Some(queue) = get_queue(data.clone(), message_component) else {
                        message_component
                            .create_response(
                                ctx.http(),
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Could not find queue to join!")
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    let response =
                        player_leave_queue(data.clone(), message_component.user.id, true, &queue);
                    message_component
                        .create_response(
                            ctx.http(),
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(response)
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    return Ok(());
                }
                if let Some(queue_id) = message_component
                    .data
                    .custom_id
                    .strip_prefix("afk_leave_queue_")
                {
                    let queue_uuid = serde_json::from_str::<QueueUuid>(queue_id)?;
                    let response = player_leave_queue(
                        data.clone(),
                        message_component.user.id,
                        true,
                        &queue_uuid,
                    );
                    message_component
                        .create_response(
                            ctx.http(),
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(response)
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    message_component.message.delete(ctx.http()).await?;
                    return Ok(());
                }
                if message_component.data.custom_id == "status" {
                    let Some(queue) = get_queue(data.clone(), message_component) else {
                        message_component
                            .create_response(
                                ctx.http(),
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Could not find queue to join!")
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    let was_in_queue = {
                        let queued_players = data.queued_players.get(&queue).unwrap();
                        queued_players.contains(&message_component.user.id)
                    };
                    let (player_state, q_entry_time) = {
                        let mut global_data = data.global_player_data.lock().unwrap();
                        let player_data = global_data.entry(message_component.user.id).or_default();
                        (
                            player_data.queue_state.clone(),
                            player_data.queue_enter_time.clone(),
                        )
                    };
                    if was_in_queue {
                        message_component
                            .create_response(
                                ctx.http(),
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!(
                                            "You'e been in queue since <t:{}:R>.",
                                            q_entry_time.unwrap().timestamp()
                                        ))
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                    } else {
                        match player_state {
                            QueueState::None => {
                                message_component
                                    .create_response(
                                        ctx.http(),
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content("You are not in queue")
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await?;
                            }
                            QueueState::Queued => {
                                message_component
                                    .create_response(
                                        ctx.http(),
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content("You are queued in a different queue.")
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await?;
                            }
                            QueueState::InGame => {
                                message_component
                                    .create_response(
                                        ctx.http(),
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content("You are in a game.")
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await?;
                            }
                        }
                    }
                    return Ok(());
                }
                if message_component.data.custom_id == "role_select" {
                    let Some(queue) = get_queue(data.clone(), message_component) else {
                        message_component
                            .create_response(
                                ctx.http(),
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Could not find queue!")
                                        .ephemeral(true),
                                ),
                            )
                            .await?;
                        return Ok(());
                    };
                    let ComponentInteractionDataKind::StringSelect { values } =
                        message_component.data.kind.clone()
                    else {
                        return Err("Invalid type for role select repsonse".into());
                    };
                    {
                        let mut players_data = data.player_data.get_mut(&queue).unwrap();
                        let player_data =
                            players_data.entry(message_component.user.id).or_default();
                        player_data.player_queueing_config.active_roles = Some(values);
                    }
                    message_component
                        .create_response(
                            ctx.http(),
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Roles configured!")
                                    .ephemeral(true),
                            ),
                        )
                        .await?;
                    return Ok(());
                }
            }
        }
        serenity::FullEvent::Message { new_message } => {
            let Some(guild_id) = new_message.guild_id else {
                return Ok(());
            };
            for queue in data
                .guild_data
                .lock()
                .unwrap()
                .entry(guild_id)
                .or_default()
                .queues
                .iter()
            {
                if data.configuration.get(queue).unwrap().log_chats {
                    let Some(match_id) = data
                        .match_channels
                        .lock()
                        .unwrap()
                        .get(&new_message.channel_id)
                        .cloned()
                    else {
                        continue;
                    };
                    fs::create_dir_all("match_logs")?;
                    let mut file = OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(format!("match_logs/match-{}.log", match_id))
                        .unwrap();
                    if let Err(e) = writeln!(
                        file,
                        "{}:{}",
                        new_message.author.mention(),
                        new_message.content.clone(),
                    ) {
                        eprintln!("Couldn't write to file: {}", e);
                    }
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

fn get_queue(data: Arc<Data>, message_component: &ComponentInteraction) -> Option<QueueUuid> {
    let queues = data
        .guild_data
        .lock()
        .unwrap()
        .entry(message_component.guild_id.unwrap())
        .or_default()
        .queues
        .clone();
    let queue = queues
        .iter()
        .filter(|queue| {
            data.clone()
                .configuration
                .get(&queue)
                .unwrap()
                .queue_messages
                .iter()
                .any(|(message_channel, message_id, _)| {
                    *message_channel == message_component.channel.clone().unwrap().id
                        && *message_id == message_component.message.id
                })
        })
        .last()
        .cloned();
    queue
}

async fn update_queue_messages(
    data: Arc<Data>,
    http: Arc<Http>,
    queue: &QueueUuid,
) -> Result<(), Error> {
    let in_game_player_count = data.current_games.get(queue).unwrap().len() * {
        let config = data.configuration.get(queue).unwrap();
        (config.team_count * config.team_size) as usize
    };
    let response = {
        let queued_players = data.queued_players.get(queue).unwrap();
        format!(
            "## Matchmaking Queue\n### {} people are playing right now\nThere are {} queued players: {}",
            queued_players.len() + in_game_player_count,
            queued_players.len(),
            queued_players.iter().map(|c| c.mention()).join(", ")
        )
    };
    let queue_messages = data
        .configuration
        .get(queue)
        .unwrap()
        .queue_messages
        .clone();
    for (message_channel, queue_message, _) in queue_messages
        .iter()
        .filter(|(_, _, message_type)| matches!(message_type, QueueMessageType::Queue))
    {
        message_channel
            .edit_message(
                http.clone(),
                queue_message,
                EditMessage::new().content(&response),
            )
            .await?;
    }
    Ok(())
}

fn log_match_results(_data: Arc<Data>, result: &MatchResult, match_data: &MatchData) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("games.log")
        .unwrap();
    if let Err(e) = writeln!(
        file,
        "match {}:{:?}\nresult:{}",
        match_data.name, match_data, result
    ) {
        eprintln!("Couldn't write to file: {}", e);
    }
}

fn log_command(ctx: &Context) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("commands.log")
        .unwrap();
    if let Err(e) = writeln!(
        file,
        "{}: {}",
        ctx.author().mention(),
        ctx.invocation_string(),
    ) {
        eprintln!("Couldn't write to file: {}", e);
    }
}

fn log_interaction(interaction: &ComponentInteraction) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open("commands.log")
        .unwrap();
    if let Err(e) = writeln!(
        file,
        "{}: {}",
        interaction.user.mention(),
        interaction.data.custom_id,
    ) {
        eprintln!("Couldn't write to file: {}", e);
    }
}

fn apply_match_results(
    data: Arc<Data>,
    result: MatchResult,
    players: &Vec<Vec<UserId>>,
    queue_id: QueueUuid,
) {
    let rating_config: WengLinConfig = WengLinConfig::default();
    if matches!(result, MatchResult::Cancel) {
        return;
    }
    let system = <WengLin as MultiTeamRatingSystem>::new(rating_config);
    let mut player_data = data.player_data.get_mut(&queue_id).unwrap();
    let config = data.configuration.get(&queue_id).unwrap();
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
    let rating_result = MultiTeamRatingSystem::rate(
        &system,
        outcome
            .iter()
            .map(|(t, o)| (t.as_slice(), o.clone()))
            .collect_vec()
            .as_slice(),
    );
    for (team_idx, team) in players.iter().enumerate() {
        for (player_idx, player) in team.iter().enumerate() {
            let player = player_data.get_mut(player).unwrap();
            player.rating = Some(
                rating_result
                    .get(team_idx)
                    .unwrap()
                    .get(player_idx)
                    .unwrap()
                    .clone(),
            );
            match result {
                MatchResult::Team(idx) if idx == team_idx as u32 => player.stats.wins += 1,
                MatchResult::Team(_) => player.stats.losses += 1,
                MatchResult::Tie => player.stats.draws += 1,
                MatchResult::Cancel => panic!("Invalid state"),
            }
        }
    }
}

async fn matchmake(
    data: Arc<Data>,
    http: Arc<Http>,
    guild_id: GuildId,
    queue_id: &QueueUuid,
) -> Result<(), Error> {
    {
        let mut guard = data.is_matchmaking.get_mut(&queue_id).unwrap();

        if guard.is_some() {
            // If already running, return
            return Ok(());
        }

        // Mark as running
        *guard = Some(());
    }

    loop {
        // Actual task execution
        let result = try_matchmaking(data.clone(), http.clone(), guild_id, queue_id).await?;

        if let Some(delay) = result {
            // Task failed, clear running state and retry after delay
            *data.is_matchmaking.get_mut(&queue_id).unwrap() = None;
            tokio::time::sleep(Duration::from_secs_f32(delay)).await;
            let mut guard = data.is_matchmaking.get_mut(&queue_id).unwrap();

            // If re-executed during sleep, exit loop
            if guard.is_some() {
                break;
            }

            // Mark as running again
            *guard = Some(());
        } else {
            data.message_edit_notify
                .get(&queue_id)
                .unwrap()
                .notify_one();
            break;
        }
    }

    // Clear running state when done
    *data.is_matchmaking.get_mut(&queue_id).unwrap() = None;
    Ok(())
}

async fn try_matchmaking(
    data: Arc<Data>,
    cache_http: Arc<Http>,
    guild_id: GuildId,
    queue_id: &QueueUuid,
) -> Result<Option<f32>, Error> {
    let (team_count, total_player_count) = {
        let configuration = data.configuration.get(&queue_id).unwrap();
        let queued_players = data.queued_players.get(&queue_id).unwrap();
        let total_player_count = configuration.team_count * configuration.team_size;
        if (queued_players.len() as u32) < total_player_count {
            return Ok(None);
        }
        (configuration.team_count, total_player_count)
    };
    let config = {
        let config = data.configuration.get(&queue_id).unwrap();
        config.clone()
    };
    let Some(category) = config.category else {
        return Err(Error::from("No category"));
    };
    let mut queued_players = data.queued_players.get(&queue_id).unwrap().clone();
    {
        let bans = data.player_bans.get(&queue_id).unwrap();
        queued_players.retain(|p| !bans.contains_key(p));
    }
    println!("Trying matchmaking");
    let members = greedy_matchmaking(data.clone(), queued_players, queue_id);
    let Some(members) = members else {
        println!("Could not find valid matchmaking");
        let delay = 10.0;
        return Ok(Some(delay));
    };
    let player_game_data = {
        let player_data = data.player_data.get(&queue_id).unwrap();
        members
            .iter()
            .map(|team| {
                team.iter()
                    .map(|player| player_data.get(player).unwrap().clone())
                    .collect_vec()
            })
            .collect_vec()
    };
    let global_player_data = {
        let player_data = data.global_player_data.lock().unwrap();
        members
            .iter()
            .map(|team| {
                team.iter()
                    .map(|player| player_data.get(player).unwrap().clone())
                    .collect_vec()
            })
            .collect_vec()
    };
    let LobbyEvaluation {
        cost: cost_eval,
        game_categories: match_categories,
        lobby_host: host,
        roles: game_roles,
    } = evaluate_cost(
        data.clone(),
        &members,
        &player_game_data,
        &global_player_data,
        queue_id,
    );
    let game_roles = game_roles
        .iter()
        .map(|team_roles| {
            team_roles
                .iter()
                .map(|game_role| {
                    config
                        .roles
                        .get(game_role)
                        .map(|role| role.name.clone())
                        .unwrap()
                })
                .collect_vec()
        })
        .collect_vec();
    if cost_eval > config.maximum_queue_cost {
        println!("Best option has cost of {}", cost_eval);
        let delay = (cost_eval - config.maximum_queue_cost) / total_player_count as f32 + 1.0;
        return Ok(Some(delay));
    }
    let new_idx = {
        let mut queue_idx = data.queue_idx.get_mut(&queue_id).unwrap();
        *queue_idx += 1;
        *queue_idx
    };
    let new_id = MatchUuid::new();

    {
        let mut global_data = data.global_player_data.lock().unwrap();
        for team in members.iter() {
            for player in team {
                data.queued_players
                    .get_mut(&queue_id)
                    .unwrap()
                    .remove(player);
                let global_data = global_data.get_mut(player).unwrap();
                global_data.queue_enter_time = None;
                global_data.queue_state = QueueState::InGame;
            }
        }
    }
    let permissions = members
        .iter()
        .flat_map(|t| t)
        .map(|user| PermissionOverwrite {
            deny: Permissions::empty(),
            allow: Permissions::VIEW_CHANNEL,
            kind: PermissionOverwriteType::Member(user.clone()),
        })
        .chain([
            PermissionOverwrite {
                deny: Permissions::VIEW_CHANNEL,
                allow: Permissions::empty(),
                kind: PermissionOverwriteType::Role(guild_id.everyone_role()),
            },
            PermissionOverwrite {
                deny: Permissions::empty(),
                allow: Permissions::VIEW_CHANNEL,
                kind: PermissionOverwriteType::Member(cache_http.get_current_user().await?.id),
            },
        ])
        .chain(
            data.configuration
                .get(&queue_id)
                .unwrap()
                .visability_override_roles
                .iter()
                .map(|role| PermissionOverwrite {
                    deny: Permissions::empty(),
                    allow: Permissions::VIEW_CHANNEL,
                    kind: PermissionOverwriteType::Role(role.clone()),
                }),
        )
        .collect_vec();
    let (match_channel, vc_channels) = future::join(
        CreateChannel::new(format!("match-{}", new_idx))
            .category(category.clone())
            .permissions(permissions.clone())
            .execute(cache_http.clone(), guild_id),
        future::join_all((0..team_count).map(|i| {
            CreateChannel::new(format!("Team {} - #{}", i + 1, new_idx))
                .category(category.clone())
                .permissions(permissions.clone())
                .kind(ChannelType::Voice)
                .execute(cache_http.clone(), guild_id)
        })),
    )
    .await;
    let match_channel = match_channel?;
    let vc_channels = vc_channels.into_iter().map(|c| c.unwrap()).collect_vec();
    let members_copy = members.clone();
    let vc_channels_copy = vc_channels.clone();
    let cache_http_copy = cache_http.clone();
    future::join(
        async {
            let mut members_message = String::new();
            members_message += format!("# Queue#{}\n", new_idx).as_str();
            for (category_name, value) in match_categories {
                members_message += format!(
                    "{}: {}\n",
                    category_name,
                    config.game_categories[&category_name][value].mention()
                )
                .as_str();
            }
            if let Some(previous_members) = host
                .map(|host| get_previous_game_members(&data, queue_id, host))
                .flatten()
            {
                let sorted_members: Vec<Vec<(UserId, String, bool)>> = previous_members
                    .iter()
                    .zip(
                        members_copy
                            .iter()
                            .zip(game_roles.iter())
                            .map(|(members, roles)| members.iter().zip(roles.iter())),
                    )
                    .map(|(prev_team, new_team)| {
                        let mut sorted_team = prev_team
                            .iter()
                            .map(|member| {
                                new_team
                                    .clone()
                                    .find(|(id, _)| id == &member)
                                    .map(|(id, role)| (id.clone(), role.clone(), true))
                            })
                            .collect_vec();
                        let mut remaining_members = new_team
                            .filter(|member| {
                                sorted_team
                                    .iter()
                                    .filter_map(|sorted_member| {
                                        sorted_member.as_ref().map(
                                            |(sorted_member, sorted_role, _)| {
                                                (sorted_member, sorted_role)
                                            },
                                        )
                                    })
                                    .find(|(id, _)| id == &member.0)
                                    .is_none()
                            })
                            .collect_vec();
                        for member in sorted_team.iter_mut() {
                            if matches!(member, None) {
                                let (remaining_member_id, remaining_member_role) =
                                    remaining_members.pop().unwrap();
                                *member = Some((
                                    remaining_member_id.clone(),
                                    remaining_member_role.clone(),
                                    false,
                                ));
                            }
                        }
                        assert!(remaining_members.is_empty());
                        assert!(sorted_team.iter().all(|member| member.is_some()));
                        sorted_team.into_iter().map(Option::unwrap).collect_vec()
                    })
                    .collect_vec();

                for (team_idx, team) in sorted_members.iter().enumerate() {
                    members_message += format!("## Team {}\n", team_idx + 1).as_str();
                    let team_copy = team.clone();
                    for (player, role, unchanged) in team_copy {
                        members_message += format!(
                            "{} {} {}\n",
                            player.mention(),
                            role,
                            if unchanged { "" } else { "*" }
                        )
                        .as_str();
                    }
                }
            } else {
                for (team_idx, team) in members_copy
                    .iter()
                    .zip(game_roles.iter())
                    .map(|(members, roles)| members.iter().zip(roles.iter()))
                    .enumerate()
                {
                    members_message += format!("## Team {}\n", team_idx + 1).as_str();
                    let team_copy = team.clone();
                    for (player, role) in team_copy {
                        members_message += format!("{} {}\n", player.mention(), role).as_str();
                    }
                }
            }
            if let Some(host) = host {
                members_message += format!("## Host: {}\n", host.mention()).as_str();
            }
            let mut message = CreateMessage::default()
                .allowed_mentions(
                    CreateAllowedMentions::default()
                        .all_roles(false)
                        .all_users(true),
                )
                .content(members_message);
            if host.is_none() {
                message = message.button(
                    CreateButton::new("volunteer_host")
                        .label("Volunteer to host")
                        .style(serenity::ButtonStyle::Primary),
                );
            }
            let members_message_id = match_channel
                .send_message(cache_http_copy.clone(), message)
                .await?;
            match_channel
                .pin(cache_http_copy.clone(), members_message_id.id)
                .await
                .ok();
            let mut map_vote_end_time = None;
            let mut map_pool = config.maps.iter().collect_vec();
            if config.prevent_recent_maps {
                let previous_maps: HashSet<String> = members_copy
                    .iter()
                    .flat_map(|t| t)
                    .flat_map(|member| get_previous_map(&data, queue_id, *member))
                    .sorted()
                    .dedup()
                    .take(map_pool.len() - config.map_vote_count as usize)
                    .collect();
                map_pool.retain(|m| !previous_maps.contains(*m));
            }
            if config.map_vote_count > 0 {
                let mut map_vote_message_content = "# Map Vote".to_string();
                if config.map_vote_time > 0 {
                    map_vote_end_time = Some(
                        std::time::UNIX_EPOCH.elapsed().unwrap().as_secs()
                            + config.map_vote_time as u64,
                    );
                    map_vote_message_content +=
                        format!("\nEnds <t:{}:R>", map_vote_end_time.unwrap()).as_str();
                }
                let mut map_vote_message =
                    CreateMessage::default().content(map_vote_message_content);
                let vote_maps = map_pool
                    .choose_multiple(&mut rand::thread_rng(), config.map_vote_count as usize)
                    .map(|m| *m)
                    .cloned()
                    .collect_vec();
                for rand_map in vote_maps.iter() {
                    map_vote_message = map_vote_message.button(
                        CreateButton::new(format!("map_{}", rand_map).clone())
                            .label(rand_map)
                            .style(serenity::ButtonStyle::Secondary),
                    );
                }
                let mut map_message = match_channel
                    .send_message(cache_http_copy.clone(), map_vote_message)
                    .await?;
                if config.map_vote_time > 0 {
                    let ctx1 = Arc::clone(&cache_http_copy);
                    let data = data.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(config.map_vote_time as u64)).await;
                        if map_message.components.is_empty() {
                            return;
                        }
                        let vote_result = {
                            let match_data = data.match_data.lock().unwrap();
                            let Some(match_data) = match_data.get(&new_id) else {
                                return;
                            };
                            match_data
                                .map_votes
                                .iter()
                                .counts_by(|(_, vote)| vote)
                                .iter()
                                .max_by_key(|(_category, vote_count)| *vote_count)
                                .map(|(category, _vote_count)| (*category).clone())
                                .unwrap_or(vote_maps.first().unwrap().clone())
                                .clone()
                        };
                        let content = format!("# Map: {}", vote_result);

                        map_message
                            .edit(
                                ctx1.clone(),
                                EditMessage::new().components(vec![]).content(content),
                            )
                            .await
                            .ok();
                    });
                }
            } else if config.maps.len() > 0 {
                let chosen_map = map_pool.choose(&mut rand::thread_rng()).unwrap();
                let map_vote_message =
                    CreateMessage::default().content(format!("# Map: {}", chosen_map));
                match_channel
                    .send_message(cache_http_copy.clone(), map_vote_message)
                    .await?;
            }
            let mut result_message = CreateMessage::default();
            for i in 0..team_count {
                result_message = result_message.button(
                    CreateButton::new(format!("team_{}", i))
                        .label(format!("Team {}", i + 1))
                        .style(serenity::ButtonStyle::Primary),
                )
            }
            match_channel
                .send_message(
                    cache_http_copy.clone(),
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
                channels.insert(match_channel.id, new_id);
            }
            {
                let mut match_data = data.match_data.lock().unwrap();
                let channels = vc_channels_copy
                    .iter()
                    .map(|c| c.id)
                    .chain([match_channel.id])
                    .collect_vec();
                match_data.insert(
                    new_id,
                    MatchData {
                        result_votes: HashMap::new(),
                        channels,
                        members: members_copy,
                        host,
                        map_votes: HashMap::new(),
                        map_vote_end_time,
                        match_end_time: None,
                        resolved: false,
                        name: format!("#{}", new_idx),
                        queue: queue_id.clone(),
                    },
                );
            }
            Ok::<(), Error>(())
        },
        async move {
            future::join_all(
                members
                    .into_iter()
                    .enumerate()
                    .map(|(team_idx, team)| {
                        (
                            vc_channels.get(team_idx.clone()).unwrap(),
                            team,
                            cache_http.clone(),
                        )
                    })
                    .map(|(team_vc, team, http)| async move {
                        future::join_all(
                            team.into_iter()
                                .map(|player| (team_vc, player, http.clone()))
                                .map(|(team_vc, player, http)| async move {
                                    guild_id.move_member(http, player, team_vc.id).await
                                }),
                        )
                        .await;
                    }),
            )
            .await;
        },
    )
    .await
    .0?;
    Ok(None)
}

fn get_previous_game_members(
    data: &Arc<Data>,
    queue_id: &QueueUuid,
    host: UserId,
) -> Option<Vec<Vec<UserId>>> {
    let host_last_game = data
        .player_data
        .get(queue_id)
        .unwrap()
        .get(&host)
        .unwrap()
        .game_history
        .last()
        .cloned();
    host_last_game.map(|host_last_game| {
        data.historical_match_data
            .lock()
            .unwrap()
            .get(&host_last_game)
            .unwrap()
            .members
            .clone()
    })
}

fn get_previous_map(data: &Arc<Data>, queue_id: &QueueUuid, player: UserId) -> Option<String> {
    let Some(player_last_game) = data
        .player_data
        .get(queue_id)
        .unwrap()
        .get(&player)
        .unwrap()
        .game_history
        .last()
        .cloned()
    else {
        return None;
    };
    data.historical_match_data
        .lock()
        .unwrap()
        .get(&player_last_game)
        .unwrap()
        .map_votes
        .iter()
        .into_group_map_by(|vote| vote.1)
        .into_iter()
        .sorted_by_key(|votes| votes.1.len())
        .map(|votes| votes.0)
        .next()
        .cloned()
}

struct LobbyEvaluation {
    cost: f32,
    game_categories: HashMap<String, usize>,
    lobby_host: Option<UserId>,
    roles: Vec<Vec<String>>,
}

fn evaluate_cost(
    data: Arc<Data>,
    player_ids: &Vec<Vec<UserId>>,
    player_data: &Vec<Vec<DerivedPlayerData>>,
    global_player_data: &Vec<Vec<GlobalPlayerData>>,
    queue_id: &QueueUuid,
) -> LobbyEvaluation {
    let (
        team_size,
        game_categories,
        default_player_data,
        max_lobby_keep_time,
        role_combinations,
        incorrect_roles_cost,
    ) = {
        let config = data.configuration.get(&queue_id).unwrap();
        (
            config.team_size,
            config.game_categories.clone(),
            config.default_player_data.clone(),
            config.max_lobby_keep_time.clone(),
            config.role_combinations.clone(),
            config.incorrect_roles_cost,
        )
    };

    let (host_cost, lobby_host) = {
        let historical_matches = data.historical_match_data.lock().unwrap();
        let current_time = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
        let costs_and_last_hosts = player_data
            .iter()
            .flat_map(|team| {
                team.iter()
                    .filter_map(|player| {
                        player.game_history.last().map(|history| {
                            (
                                player.player_queueing_config.new_lobby_host_cost.unwrap_or(
                                    default_player_data
                                        .player_queueing_config
                                        .new_lobby_host_cost,
                                ),
                                history,
                            )
                        })
                    })
                    .filter_map(|(wrong_host_cost, game_id)| {
                        historical_matches
                            .get(game_id)
                            .map(|game| (wrong_host_cost, game))
                    })
                    .filter(|(_, game)| {
                        if let Some(end_time) = game.match_end_time {
                            current_time - end_time <= max_lobby_keep_time
                        } else {
                            false
                        }
                    })
                    .filter_map(|(wrong_host_cost, game)| {
                        game.host.map(|host| (wrong_host_cost, host))
                    })
            })
            .collect_vec();
        let lobby_host = costs_and_last_hosts
            .iter()
            .map(|(_, host)| host)
            .filter(|host| {
                player_ids
                    .iter()
                    .flat_map(|team| team.iter())
                    .contains(host)
            })
            .counts()
            .iter()
            .max_by(|(_host, count), (_host2, count2)| count.cmp(count2))
            .map(|(host, _count)| *host)
            .cloned();
        let host_cost = if let Some(lobby_host) = lobby_host {
            costs_and_last_hosts
                .iter()
                .filter(|(_, host)| host != &lobby_host)
                .map(|(cost, _)| cost)
                .sum::<f32>()
                - data
                    .player_data
                    .get(queue_id)
                    .unwrap()
                    .get(&lobby_host)
                    .unwrap()
                    .player_queueing_config
                    .new_lobby_host_cost
                    .unwrap_or(
                        default_player_data
                            .player_queueing_config
                            .new_lobby_host_cost,
                    )
        } else {
            0.0
        };
        (host_cost, lobby_host)
    };
    let team_mmrs = player_data.iter().map(|team| {
        team.iter()
            .map(|player| player.rating.unwrap_or(default_player_data.rating).rating as f32)
            .sum::<f32>()
            / team_size as f32
    });
    let team_mmr_stds = player_data
        .iter()
        .zip(team_mmrs.clone())
        .map(|(team, team_mmr)| {
            team.iter()
                .map(|player| {
                    player.rating.unwrap_or(default_player_data.rating).rating as f32 - team_mmr
                })
                .map(|rating| rating * rating)
                .sum::<f32>()
                / team_size as f32
        })
        .map(|team_variance| team_variance.sqrt());
    let mmr_differential = match team_mmrs.minmax() {
        MinMaxResult::NoElements => 0.0,
        MinMaxResult::OneElement(_) => 0.0,
        MinMaxResult::MinMax(min, max) => max - min,
    };
    let mmr_std_differential = match team_mmr_stds.minmax() {
        MinMaxResult::NoElements => 0.0,
        MinMaxResult::OneElement(_) => 0.0,
        MinMaxResult::MinMax(min, max) => max - min,
    };
    let mmr_range = player_data
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

    let player_categories: Vec<HashMap<String, Vec<usize>>> = player_data
        .iter()
        .flat_map(|team| team.iter().map(|player| player.game_categories.clone()))
        .collect_vec();
    let game_categories: HashMap<String, usize> = game_categories
        .iter()
        .map(|(category_name, _)| {
            (
                category_name.clone(),
                player_categories
                    .iter()
                    .map(|player_categories| player_categories[category_name].clone())
                    .flatten()
                    .counts()
                    .into_iter()
                    .max_by_key(|&(_category, count)| count)
                    .map(|(category, _count)| category)
                    .unwrap_or(0),
            )
        })
        .collect();

    let mut role_cost = 0.0;
    let roles = player_data
        .iter()
        .map(|team| {
            let team_roles = team
                .iter()
                .map(|player| {
                    player
                        .player_queueing_config
                        .active_roles
                        .as_ref()
                        .unwrap_or(&default_player_data.player_queueing_config.active_roles)
                        .clone()
                })
                .collect_vec();
            for (role_combination, combination_cost) in role_combinations.iter() {
                let edges = team_roles
                    .iter()
                    .enumerate()
                    .flat_map(|(player_idx, roles)| {
                        roles
                            .iter()
                            .flat_map(|player_role_id| {
                                role_combination
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, combination_role_id)| {
                                        player_role_id == *combination_role_id
                                    })
                                    .map(|(role_idx, _)| {
                                        (player_idx, role_idx + team_size as usize)
                                    })
                                    .collect_vec()
                            })
                            .collect_vec()
                    })
                    .collect_vec();
                let correct_edges = matching(&edges)
                    .into_iter()
                    .sorted_by_key(|(player_idx, _)| *player_idx)
                    .collect_vec();
                if correct_edges.len() == team_roles.len().min(role_combination.len()) {
                    role_cost += combination_cost;
                    return correct_edges
                        .iter()
                        .map(|(_, role_idx)| role_idx - team_size as usize)
                        .map(|role_idx| role_combination[role_idx].clone())
                        .collect_vec();
                }
            }
            role_cost += incorrect_roles_cost;
            team_roles.iter().map(|_| "".to_string()).collect_vec()
        })
        .collect_vec();

    let now = chrono::offset::Utc::now();
    let cost = host_cost
        + role_cost
        + player_data
            .iter()
            .flat_map(|team| team.iter())
            .zip(global_player_data.iter().flat_map(|team| team.iter()))
            .zip(player_categories.iter())
            .map(|((player, global_player), player_categories)| {
                let queue_config = player
                    .player_queueing_config
                    .derive(&default_player_data.player_queueing_config);
                let time_in_queue = global_player
                    .queue_enter_time
                    .map(|queue_time| (now - queue_time).num_seconds())
                    .unwrap_or(0);
                (mmr_differential - queue_config.acceptable_mmr_differential).max(0.0)
                    * queue_config.cost_per_avg_mmr_differential
                    + (mmr_std_differential - queue_config.acceptable_mmr_std_differential).max(0.0)
                        * queue_config.cost_per_mmr_std_differential
                    + (mmr_range - queue_config.acceptable_mmr_range).max(0.0)
                        * queue_config.cost_per_mmr_range
                    + queue_config
                        .wrong_game_category_cost
                        .iter()
                        .filter(|(category, _)| {
                            !player_categories[*category].contains(&game_categories[*category])
                        })
                        .map(|(_, cost)| cost)
                        .sum::<f32>()
                    - time_in_queue as f32
            })
            .sum::<f32>();
    LobbyEvaluation {
        cost,
        game_categories,
        lobby_host,
        roles,
    }
}

fn greedy_matchmaking(
    data: Arc<Data>,
    pool: HashSet<UserId>,
    queue_id: &QueueUuid,
) -> Option<Vec<Vec<UserId>>> {
    let team_size = data.configuration.get(&queue_id).unwrap().team_size;
    let team_count = data.configuration.get(&queue_id).unwrap().team_count;
    let total_players = team_size * team_count;
    let mut players = pool.clone();
    let mut result = vec![vec![]; team_count as usize];
    let mut player_count = 0;

    while player_count < total_players {
        println!("Player count: {}", player_count);
        let mut min_cost = f32::MAX;
        let mut best_next_result = vec![];
        let mut best_added_players = vec![];
        'additions_loop: for possible_addition in players.iter() {
            for team_idx in 0..team_count as usize {
                if result[team_idx].len() >= team_size as usize {
                    continue;
                }
                let mut result_copy = result.clone();
                let mut added_players = vec![];
                if let Some(party) = data
                    .global_player_data
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
                        if !players.contains(player) {
                            continue 'additions_loop;
                        }
                        added_players.push(player.clone());
                        result_copy[team_idx].push(player.clone());
                    }
                } else {
                    added_players.push(possible_addition.clone());
                    result_copy[team_idx].push(possible_addition.clone());
                }

                let player_game_data = {
                    let mut player_data = data.player_data.get_mut(&queue_id).unwrap();
                    result_copy
                        .iter()
                        .map(|team| {
                            team.iter()
                                .map(|player| player_data.entry(*player).or_default().clone())
                                .collect_vec()
                        })
                        .collect_vec()
                };
                let global_player_data = {
                    let player_data = data.global_player_data.lock().unwrap();
                    result_copy
                        .iter()
                        .map(|team| {
                            team.iter()
                                .map(|player| player_data.get(player).unwrap().clone())
                                .collect_vec()
                        })
                        .collect_vec()
                };
                let cost = evaluate_cost(
                    data.clone(),
                    &result_copy,
                    &player_game_data,
                    &global_player_data,
                    queue_id,
                )
                .cost;
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

/// Exports configuration
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn backup(ctx: Context<'_>) -> Result<(), Error> {
    {
        let time_stamp = chrono::offset::Utc::now().naive_utc();
        let config = serde_json::to_string_pretty(ctx.data())?;
        println!("Starting backup...");
        fs::write(
            format!(
                "backups/backup_{}.json",
                time_stamp.format("%Y_%m_%d_%H_%M_%S")
            ),
            config,
        )?;
        println!("Backup made!");
    }
    let response = format!("Backup made.");
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Join queue
#[poise::command(slash_command, prefix_command)]
async fn queue(ctx: Context<'_>) -> Result<(), Error> {
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    let Some(queue) = queues.iter().last() else {
        ctx.send(
            CreateReply::default()
                .content("Could not find queue to join!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    match try_queue_player(
        ctx.data().clone(),
        queue,
        ctx.author().id,
        ctx.serenity_context().http.clone(),
        ctx.guild_id().unwrap(),
        true,
        false,
    )
    .await
    {
        Ok(()) => {
            let response = {
                let data_lock = ctx.data().queued_players.get(queue).unwrap();
                format!(
                    "Queued players: {}",
                    data_lock.iter().map(|c| c.mention()).join(", ")
                )
            };
            ctx.send(CreateReply::default().content(response).ephemeral(true))
                .await?;
            ctx.data()
                .message_edit_notify
                .get(queue)
                .unwrap()
                .notify_one();
            matchmake(
                ctx.data().clone(),
                ctx.serenity_context().http.clone(),
                ctx.guild_id().unwrap(),
                queue,
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

/// Add fake players to queue to stress test
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn queue_many(ctx: Context<'_>, count: u32) -> Result<(), Error> {
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    let Some(queue) = queues.iter().last() else {
        ctx.send(
            CreateReply::default()
                .content("Could not find queue to join!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    for i in 0..count {
        match try_queue_player(
            ctx.data().clone(),
            queue,
            UserId::new((i + 1) as u64),
            ctx.serenity_context().http.clone(),
            ctx.guild_id().unwrap(),
            true,
            true,
        )
        .await
        {
            Ok(()) => {}
            Err(reason) => {
                ctx.send(CreateReply::default().content(reason).ephemeral(true))
                    .await?;
                return Ok(());
            }
        }
    }
    let response = {
        let data_lock = ctx.data().queued_players.get(queue).unwrap();
        format!(
            "Queued players: {}",
            data_lock.iter().map(|c| c.mention()).join(", ")
        )
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    ctx.data()
        .message_edit_notify
        .get(queue)
        .unwrap()
        .notify_one();
    Ok(())
}

fn player_leave_queue(
    data: Arc<Data>,
    user: UserId,
    queue_group: bool,
    queue: &QueueUuid,
) -> String {
    if queue_group {
        let possible_party = data
            .global_player_data
            .lock()
            .unwrap()
            .entry(user.clone())
            .or_insert(GlobalPlayerData::default())
            .party
            .map(|party| {
                data.group_data
                    .lock()
                    .unwrap()
                    .get(&party)
                    .map(|p| p.players.clone())
            });
        if let Some(Some(party_members)) = possible_party {
            for user in party_members {
                player_leave_queue(data.clone(), user, false, queue);
            }
            return "Party left queue".to_string();
        }
    }
    let removed = {
        let mut queued_players = data.queued_players.get_mut(queue).unwrap();
        queued_players.remove(&user)
    };
    if removed {
        {
            let mut player_data = data.global_player_data.lock().unwrap();
            let global_player_data = player_data
                .entry(user.clone())
                .or_insert(GlobalPlayerData::default());
            global_player_data.queue_enter_time = None;
            global_player_data.queue_state = QueueState::None;
        }
        data.message_edit_notify
            .get_mut(queue)
            .unwrap()
            .notify_one();
        "You are no longer queueing!".to_string()
    } else {
        "You weren't queued!".to_string()
    }
}

/// Join queue
#[poise::command(slash_command, prefix_command)]
async fn leave_queue(ctx: Context<'_>) -> Result<(), Error> {
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    for queue in queues {
        let response = player_leave_queue(ctx.data().clone(), ctx.author().id, true, &queue);
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
    }
    Ok(())
}

/// Lists queued players
#[poise::command(slash_command, prefix_command)]
async fn list_queued(ctx: Context<'_>) -> Result<(), Error> {
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    for queue in queues {
        let response = {
            let data_lock = ctx.data().queued_players.get(&queue).unwrap();
            format!(
                "There are {} queued players: {}",
                data_lock.len(),
                data_lock.iter().map(|c| c.mention()).join(", ")
            )
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
    }
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

/// Shows player stats
#[poise::command(slash_command, prefix_command)]
async fn stats(
    ctx: Context<'_>,
    #[description = "User to get stats for"] user: Option<serenity::UserId>,
) -> Result<(), Error> {
    let user = user.unwrap_or(ctx.author().id);
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    for queue in queues {
        let (stats, rating) = {
            let mut player_data = ctx.data().player_data.get_mut(&queue).unwrap();
            let config = ctx.data().configuration.get(&queue).unwrap();
            let player_data = player_data
                .entry(user)
                .or_insert(DerivedPlayerData::default());
            (
                player_data.stats.clone(),
                player_data
                    .rating
                    .unwrap_or(config.default_player_data.rating),
            )
        };
        let response = format!(
            "{}'s mmr is {}, with uncertainty {}\nScore: {}-{}-{}",
            user.mention(),
            rating.rating,
            rating.uncertainty,
            stats.wins,
            stats.losses,
            stats.draws
        );
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
    }
    Ok(())
}

/// Invites player to party
#[poise::command(slash_command, prefix_command, rename = "invite")]
async fn party_invite(
    ctx: Context<'_>,
    #[description = "Invite player to party"] user: UserId,
) -> Result<(), Error> {
    let queue_state = ctx
        .data()
        .global_player_data
        .lock()
        .unwrap()
        .entry(ctx.author().id)
        .or_default()
        .queue_state
        .clone();
    match queue_state {
        QueueState::Queued => {
            ctx.send(
                CreateReply::default()
                    .content(format!("Cannot invite players to party while in queue"))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
        QueueState::InGame => {
            ctx.send(
                CreateReply::default()
                    .content(format!("Cannot invite players to party while in game"))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
        QueueState::None => {}
    }

    let party = {
        let mut user_data = ctx.data().global_player_data.lock().unwrap();
        let user_data = user_data.entry(ctx.author().id).or_default();
        if user_data.party.is_none() {
            user_data.party = Some(GroupUuid::new());
        }
        user_data.party.unwrap()
    };
    let user_party = {
        let mut group_data = ctx.data().group_data.lock().unwrap();
        let user_party = group_data.entry(party).or_insert(QueueGroup {
            players: HashSet::from([ctx.author().id]),
            pending_invites: HashSet::new(),
        });
        user_party.pending_invites.insert(user);
        user_party.clone()
    };
    let Ok(_) = user
        .direct_message(
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
        .await
    else {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Could not invite {} to your party. Maybe they don't have dms open?",
                    user.mention()
                ))
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
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
    old_party: GroupUuid,
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
        let mut user_data = ctx.data().global_player_data.lock().unwrap();
        let user_data = user_data
            .entry(ctx.author().id)
            .or_insert(GlobalPlayerData::default());
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
        let mut user_data = ctx.data().global_player_data.lock().unwrap();
        let user_data = user_data
            .entry(ctx.author().id)
            .or_insert(GlobalPlayerData::default());
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
    let (party_members, pending_members) = {
        let mut group_data = ctx.data().group_data.lock().unwrap();
        let user_party = group_data.get_mut(&party).unwrap();
        (
            user_party.players.clone(),
            user_party.pending_invites.clone(),
        )
    };
    let mut content = format!(
        "Party members: {}",
        party_members.iter().map(|p| p.mention()).join(", ")
    );
    if pending_members.len() > 0 {
        content += format!(
            "\nPending members: {}",
            pending_members.iter().map(|p| p.mention()).join(", ")
        )
        .as_str();
    }
    ctx.send(CreateReply::default().content(content).ephemeral(true))
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
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    for queue in queues {
        let mut player_data = ctx
            .data()
            .player_data
            .get(&queue)
            .unwrap()
            .iter()
            .map(|(id, data)| {
                (
                    id.mention(),
                    data.rating
                        .unwrap_or_else(|| {
                            ctx.data()
                                .configuration
                                .get(&queue)
                                .unwrap()
                                .default_player_data
                                .rating
                        })
                        .rating,
                )
            })
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
    }
    Ok(())
}

fn update_bans(data: Arc<Data>, queue_id: &QueueUuid) {
    let now = chrono::offset::Utc::now();
    data.player_bans.get_mut(&queue_id).unwrap().retain(
        |_,
         BanData {
             end_time,
             reason: _,
             shadow_ban: _,
         }| {
            if let Some(end_time) = end_time {
                *end_time > now
            } else {
                true
            }
        },
    )
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
        ctx.send(
            CreateReply::default()
                .content("This command must be done in a match channel!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    let match_data: MatchData = ctx
        .data()
        .match_data
        .lock()
        .unwrap()
        .get(&match_number)
        .ok_or("Could not get match data")?
        .clone();
    if !match_data
        .members
        .iter()
        .flatten()
        .contains(&ctx.author().id)
    {
        ctx.send(
            CreateReply::default()
                .content("You aren't in this match!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }
    if !match_data.members.iter().flatten().contains(&player) {
        ctx.send(
            CreateReply::default()
                .content("This player is not in this match!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }
    let leaver_message_content = format!(
        "# Did you leave {}?\nEnds <t:{}:R>, otherwise user will be reported",
        player.mention(),
        std::time::UNIX_EPOCH.elapsed().unwrap().as_secs()
            + ctx
                .data()
                .configuration
                .get_mut(&match_data.queue)
                .unwrap()
                .leaver_verification_time as u64
    );
    let leaver_message = CreateReply::default()
        .content(leaver_message_content)
        .components(vec![CreateActionRow::Buttons(vec![CreateButton::new(
            format!("leaver_check_{}", player.get()).clone(),
        )
        .label("No, I'm here.")
        .style(serenity::ButtonStyle::Primary)])]);
    let leaver_message = ctx.send(leaver_message).await?.message().await?.id;
    {
        let data = ctx.data().clone();
        let guild_id = ctx.guild_id().unwrap();
        let channel_id = ctx.channel_id();
        let ctx1 = ctx.serenity_context().http.clone();
        tokio::spawn(async move {
            let leaver_verification_time = data
                .clone()
                .configuration
                .get_mut(&match_data.queue)
                .unwrap()
                .leaver_verification_time as u64;
            tokio::time::sleep(Duration::from_secs(leaver_verification_time)).await;
            let Ok(message) = ctx1.get_message(channel_id, leaver_message).await else {
                return;
            };
            message.delete(ctx1.clone()).await.ok();
            let Ok(mut member) = guild_id.member(ctx1.clone(), player).await else {
                return;
            };
            member
                .edit(ctx1, EditMember::new().disconnect_member())
                .await
                .ok();
            *data
                .leaver_data
                .get_mut(&match_data.queue)
                .unwrap()
                .entry(player)
                .or_insert(0) += 1;
        });
    }

    Ok(())
}

/// Pings players that haven't voted
#[poise::command(slash_command, prefix_command)]
async fn ping_non_voters(ctx: Context<'_>) -> Result<(), Error> {
    let match_number = {
        let match_channels = ctx.data().match_channels.lock().unwrap();
        match_channels.get(&ctx.channel_id()).cloned()
    };
    let Some(match_number) = match_number else {
        ctx.send(
            CreateReply::default()
                .content("This command must be done in a match channel!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    let match_data: MatchData = ctx
        .data()
        .match_data
        .lock()
        .unwrap()
        .get(&match_number)
        .ok_or("Could not get match data")?
        .clone();
    if !match_data
        .members
        .iter()
        .flatten()
        .contains(&ctx.author().id)
    {
        ctx.send(
            CreateReply::default()
                .content("You aren't in this match!")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let message_content = format!(
        "# Remember to vote\n{}",
        match_data
            .members
            .iter()
            .flatten()
            .filter(|member| !match_data.result_votes.contains_key(&member))
            .map(|member| format!("{}", member.mention()))
            .join(", ")
            .as_str()
    );
    ctx.send(CreateReply::default().content(message_content))
        .await?
        .message()
        .await?;

    Ok(())
}

/// Sends a message without pinging
#[poise::command(slash_command, prefix_command)]
async fn no_ping(ctx: Context<'_>, #[rest] text: String) -> Result<(), Error> {
    ctx.send(
        CreateReply::default()
            .content(format!("{}: {}", ctx.author().mention(), text))
            .ephemeral(false)
            .allowed_mentions(CreateAllowedMentions::default().empty_roles().empty_users()),
    )
    .await?
    .into_message()
    .await?
    .id;

    Ok(())
}

/// Lists queues for this server
#[poise::command(slash_command, prefix_command)]
async fn list_queues(ctx: Context<'_>) -> Result<(), Error> {
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .or_default()
        .queues
        .clone();
    ctx.send(
        CreateReply::default()
            .content(format!("Queues: {:?}", queues))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_BOT_TOKEN").expect("missing DISCORD_BOT_TOKEN");
    let intents =
        serenity::GatewayIntents::non_privileged().union(serenity::GatewayIntents::MESSAGE_CONTENT);

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            event_handler: |ctx, event, framework, data| {
                Box::pin(handler(ctx, event, framework, data.clone()))
            },
            pre_command: |ctx| {
                Box::pin(async move {
                    log_command(&ctx);
                })
            },
            commands: vec![
                register(),
                configure(),
                backup(),
                export_config(),
                import_config(),
                queue(),
                queue_many(),
                leave_queue(),
                list_queued(),
                stats(),
                party(),
                list_parties(),
                leaderboard(),
                manage_player(),
                mark_leaver(),
                list_leavers(),
                force_outcome(),
                create_queue_message(),
                create_roles_message(),
                create_register_message(),
                no_ping(),
                player_config(),
                ping_non_voters(),
                list_queues(),
                create_queue(),
            ],
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(|_ctx, _ready, _framework| {
            Box::pin(async move {
                let config_data: Option<Arc<Data>> =
                    fs::read_to_string("config.json").ok().map(|read| {
                        serde_json::from_str(read.as_str()).expect("Failed to parse config file")
                    });
                if let Some(data) = config_data {
                    for config in data.configuration.iter() {
                        data.message_edit_notify
                            .insert(config.key().clone(), Arc::new(Notify::new()));
                    }
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
