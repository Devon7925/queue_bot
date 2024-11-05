use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use itertools::Itertools;
use poise::{
    serenity_prelude::{self as serenity, Mentionable},
    CreateReply,
};
use tokio::sync::Notify;

use crate::{Context, Error, QueueConfiguration, QueueUuid, RoleConfiguration};

fn get_queue_uuid(ctx: &Context, queue_idx: Option<u32>) -> Result<QueueUuid, String> {
    let queues = ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .clone();
    if queues.len() == 0 {
        return Err("No queues available.".to_string());
    } else if let Some(queue_idx) = queue_idx {
        if let Some(queue) = queues.get(queue_idx as usize) {
            Ok(queue.clone())
        } else {
            return Err("Invalid queue idx.".to_string());
        }
    } else if queues.len() == 1 {
        Ok(queues.get(0).unwrap().clone())
    } else {
        return Err(
            "Multiple queues available: you must specify which queue you want to use".to_string(),
        );
    }
}

macro_rules! configure_server_parameter {
    ($func_name:ident, $prop:ident, $prop_type:ty, $rename:expr, $name:expr, $doc:expr$(, $limits:meta)?) => {
#[doc=$doc]
#[poise::command(slash_command, rename=$rename)]
pub async fn $func_name(
    ctx: Context<'_>,
    #[description = "New value"]
    $(#[$limits])?
    new_value: Option<$prop_type>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(())
        }
    };
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.$prop = new_value;
        format!("{} set to {}", $name, new_value)
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!("{} is currently {}", $name, data_lock.$prop)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}
    };
}

struct ConfigurationModifiers;
impl ConfigurationModifiers {
    configure_server_parameter!(
        configure_team_size,
        team_size,
        u32,
        "team_size",
        "Team size",
        "Displays or sets team size",
        min = 1
    );
    configure_server_parameter!(
        configure_team_count,
        team_count,
        u32,
        "team_count",
        "Team count",
        "Displays or sets team count",
        min = 1
    );
    configure_server_parameter!(
        configure_map_vote_count,
        map_vote_count,
        u32,
        "map_vote_count",
        "Map vote count",
        "Displays or sets number of maps for the vote",
        min = 1
    );
    configure_server_parameter!(
        configure_map_vote_time,
        map_vote_time,
        u32,
        "map_vote_time",
        "Map vote time",
        "Displays or sets time maps for the vote (0 for no timeout)",
        min = 0
    );
    configure_server_parameter!(
        configure_maximum_queue_cost,
        maximum_queue_cost,
        f32,
        "maximum_queue_cost",
        "Max queue cost",
        "Displays or sets maximum cost it will allow for a match to be created"
    );
    configure_server_parameter!(
        configure_incorrect_roles_cost,
        incorrect_roles_cost,
        f32,
        "incorrect_roles_cost",
        "Incorrect roles cost",
        "Displays or sets cost for not assigning roles",
        min = 0
    );
    configure_server_parameter!(
        configure_log_chats,
        log_chats,
        bool,
        "log_chats",
        "Should log match chats?",
        "Displays or sets whether to log match chats"
    );
    configure_server_parameter!(
        configure_prevent_recent_maps,
        prevent_recent_maps,
        bool,
        "prevent_recent_maps",
        "Prevent recent maps?",
        "Displays or sets whether to prevent recent maps from being played"
    );
}

/// Displays or sets queue category
#[poise::command(slash_command, prefix_command, rename = "queue_category")]
async fn configure_queue_category(
    ctx: Context<'_>,
    #[description = "Queue category"]
    #[channel_types("Category")]
    new_value: Option<serenity::Channel>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = if let Some(new_value) = new_value {
        if new_value.clone().category().is_none() {
            format!(
                "Channel {} is not a category.",
                new_value.clone().to_string()
            )
        } else {
            let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
            data_lock.category = Some(new_value.id().clone());
            format!("Queue category set to {}", new_value.to_string())
        }
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
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
/// Configures queue channels
#[poise::command(slash_command, prefix_command, rename = "queue_channels")]
async fn configure_queue_channels(
    ctx: Context<'_>,
    #[flag] remove: bool,
    #[description = "Queue channel"]
    #[channel_types("Voice")]
    channel: Option<serenity::ChannelId>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        if let Some(value) = channel {
            if remove {
                if data_lock.queue_channels.remove(&value) {
                    format!("{} removed as queue channel", value)
                } else {
                    format!("{} wasn't a queue channel", value)
                }
            } else {
                data_lock.queue_channels.insert(value.clone());
                format!("{} added as queue channel", value)
            }
        } else {
            format!(
                "Queue channels are {}",
                data_lock
                    .queue_channels
                    .iter()
                    .map(|c| c.mention())
                    .join(", ")
            )
        }
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

// Displays or adds maps
#[poise::command(slash_command, prefix_command, rename = "maps")]
async fn configure_maps(
    ctx: Context<'_>,
    #[flag] remove: bool,
    #[description = "Map"] map: Option<String>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        if let Some(value) = map {
            if remove {
                if data_lock.maps.remove(&value) {
                    format!("{} removed as map", value)
                } else {
                    format!("{} wasn't a map", value)
                }
            } else {
                data_lock.maps.insert(value.clone());
                format!("{} added as map", value)
            }
        } else {
            format!("Maps are {}", data_lock.maps.iter().join(", "))
        }
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

// Displays or adds roles
#[poise::command(slash_command, prefix_command, rename = "roles")]
async fn configure_roles(
    ctx: Context<'_>,
    #[flag] remove: bool,
    #[description = "Role"] role_id: Option<String>,
    #[description = "Role name"] role_name: Option<String>,
    #[description = "Role description"] role_description: Option<String>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = 'response: {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        let Some(role_id) = role_id else {
            break 'response format!(
                "Roles:\n{}",
                data_lock
                    .roles
                    .values()
                    .map(|role| format!("* {}: {}", role.name, role.description))
                    .join("\n")
            );
        };
        if remove {
            break 'response if let Some(role) = data_lock.roles.remove(&role_id) {
                data_lock.role_combinations.retain(|(combination, _)| !combination.contains(&role_id));
                format!("{}(id: {}) removed as role", role.name, role_id)
            } else {
                format!("{} wasn't a role", role_id)
            };
        }
        if role_name.is_none() {
            break 'response "Role name missing".to_string();
        }
        data_lock.roles.insert(
            role_id.clone(),
            RoleConfiguration {
                name: role_name.unwrap(),
                description: role_description.unwrap_or("".to_string()),
            },
        );
        format!("{} added as role", role_id)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

// Displays or adds role combinations
#[poise::command(slash_command, prefix_command, rename = "role_combinations")]
async fn configure_role_combinations(
    ctx: Context<'_>,
    #[description = "Role combinations"] role_combinations: Option<String>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = if let Some(role_combinations) = role_combinations {
        let Ok(role_combinations) = serde_json::from_str::<Vec<(Vec<String>, f32)>>(&role_combinations.as_str()) else {
            ctx.send(CreateReply::default().content("Invalid combinations").ephemeral(true))
                .await?;
            return Ok(())
        };
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.role_combinations = role_combinations;
        format!(
            "Role combinations updated to:\n{}",
            data_lock
                .role_combinations
                .iter()
                .map(|(combination, cost)| format!("{:?} - {}", combination, cost))
                .join("\n")
        )
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!(
            "Role combinations:\n{}",
            data_lock
                .role_combinations
                .iter()
                .map(|(combination, cost)| format!("{:?} - {}", combination, cost))
                .join("\n")
        )
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Sets the channel to move members to after the end of the game
#[poise::command(slash_command, prefix_command, rename = "post_match_channel")]
async fn configure_post_match_channel(
    ctx: Context<'_>,
    #[description = "Post match channel"]
    #[channel_types("Voice")]
    new_value: Option<serenity::Channel>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.post_match_channel = Some(new_value.id());
        format!("Post match channel changed to {}", new_value.to_string())
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!(
            "Post match channel is {}",
            data_lock
                .post_match_channel
                .as_ref()
                .map(|c| format!("{}", c.mention()))
                .unwrap_or("not set".to_string())
        )
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Sets the channel to send logs of moderation actions to
#[poise::command(slash_command, prefix_command, rename = "audit_channel")]
async fn configure_audit_channel(
    ctx: Context<'_>,
    #[description = "Audit channel"]
    #[channel_types("Text")]
    new_value: Option<serenity::Channel>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.audit_channel = Some(new_value.id());
        format!("Audit channel changed to {}", new_value.to_string())
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!(
            "Audit channel is {}",
            data_lock
                .audit_channel
                .as_ref()
                .map(|c| format!("{}", c.mention()))
                .unwrap_or("not set".to_string())
        )
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Sets the role for registered players
#[poise::command(slash_command, prefix_command, rename = "register_role")]
async fn configure_register_role(
    ctx: Context<'_>,
    #[description = "Register role"] new_value: Option<serenity::RoleId>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.register_role = Some(new_value);
        format!("Register role changed to {}", new_value.to_string())
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!(
            "Register role is {}",
            data_lock
                .register_role
                .as_ref()
                .map(|c| format!("{}", c.mention()))
                .unwrap_or("not set".to_string())
        )
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Configures roles that can see match channels of matches their not in
#[poise::command(slash_command, prefix_command, rename = "visability_override_roles")]
async fn configure_visability_override_roles(
    ctx: Context<'_>,
    #[flag] remove: bool,
    #[description = "Override role"] channel: Option<serenity::RoleId>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let response = {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        if let Some(value) = channel {
            if remove {
                if data_lock.visability_override_roles.remove(&value) {
                    format!("{} removed as override role", value)
                } else {
                    format!("{} wasn't a override role", value)
                }
            } else {
                data_lock.visability_override_roles.insert(value.clone());
                format!("{} added as override role", value)
            }
        } else {
            format!(
                "Override roles are {}",
                data_lock
                    .visability_override_roles
                    .iter()
                    .map(|c| c.mention())
                    .join(", ")
            )
        }
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
        "ConfigurationModifiers::configure_team_size",
        "ConfigurationModifiers::configure_team_count",
        "configure_queue_category",
        "configure_queue_channels",
        "configure_post_match_channel",
        "configure_maps",
        "configure_roles",
        "configure_role_combinations",
        "ConfigurationModifiers::configure_map_vote_count",
        "ConfigurationModifiers::configure_map_vote_time",
        "ConfigurationModifiers::configure_maximum_queue_cost",
        "ConfigurationModifiers::configure_incorrect_roles_cost",
        "configure_register_role",
        "configure_audit_channel",
        "ConfigurationModifiers::configure_log_chats",
        "ConfigurationModifiers::configure_prevent_recent_maps",
        "configure_visability_override_roles",
    )
)]
pub async fn configure(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Creates a queue
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS"
)]
pub async fn create_queue(ctx: Context<'_>) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid::new();
    ctx.data()
        .configuration
        .insert(queue_uuid, QueueConfiguration::default());
    ctx.data()
        .current_games
        .insert(queue_uuid, HashSet::default());
    ctx.data()
        .is_matchmaking
        .insert(queue_uuid, Option::default());
    ctx.data()
        .leaver_data
        .insert(queue_uuid, HashMap::default());
    ctx.data()
        .message_edit_notify
        .insert(queue_uuid, Arc::new(Notify::new()));
    ctx.data().player_bans.insert(queue_uuid, HashMap::new());
    ctx.data().player_data.insert(queue_uuid, HashMap::new());
    ctx.data().queue_idx.insert(queue_uuid, 0);
    ctx.data().queued_players.insert(queue_uuid, HashSet::new());

    ctx.data()
        .guild_data
        .lock()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .or_default()
        .queues
        .push(queue_uuid);
    //ensure queue is part of server
    let response = format!("Created new queue with uuid: `{}`", queue_uuid.0);
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
pub async fn import_config(
    ctx: Context<'_>,
    #[description = "New config"] new_config: String,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let new_config: QueueConfiguration = serde_json::from_str(&new_config.as_str())?;
    *ctx.data().configuration.get_mut(&queue_uuid).unwrap() = new_config;
    let config = serde_json::to_string_pretty(ctx.data())?;
    let response = format!("Configuration set to: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Exports configuration
#[poise::command(slash_command, prefix_command)]
pub async fn export_config(
    ctx: Context<'_>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = match get_queue_uuid(&ctx, queue_idx) {
        Ok(queue_uuid) => queue_uuid,
        Err(error) => {
            ctx.send(CreateReply::default().content(error).ephemeral(true))
                .await?;
            return Ok(());
        }
    };
    let config =
        serde_json::to_string_pretty(&ctx.data().configuration.get(&queue_uuid).unwrap().clone())?;
    let response = format!("Configuration: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}
