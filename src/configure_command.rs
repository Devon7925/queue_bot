use std::{collections::{HashMap, HashSet}, sync::Arc};

use itertools::Itertools;
use poise::{
    serenity_prelude::{self as serenity, Mentionable},
    CreateReply,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{Context, Error, QueueConfiguration, QueueUuid};

/// Displays or sets team size
#[poise::command(slash_command, prefix_command, rename = "team_size")]
async fn configure_team_size(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "New value"]
    #[min = 1]
    new_value: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.team_size = new_value;
        format!("Team size set to {}", new_value)
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!("Team size is currently {}", data_lock.team_size)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays or sets team count
#[poise::command(slash_command, prefix_command, rename = "team_count")]
async fn configure_team_count(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "New value"]
    #[min = 1]
    new_value: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.team_count = new_value;
        format!("Team count set to {}", new_value)
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!("Team count is currently {}", data_lock.team_count)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays or sets queue category
#[poise::command(slash_command, prefix_command, rename = "queue_category")]
async fn configure_queue_category(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "Queue category"]
    #[channel_types("Category")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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
    #[description = "Queue to change"] queue: String,
    #[flag] remove: bool,
    #[description = "Queue channel"]
    #[channel_types("Voice")]
    channel: Option<serenity::ChannelId>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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
    #[description = "Queue to change"] queue: String,
    #[flag] remove: bool,
    #[description = "Map"] map: Option<String>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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

/// Displays or sets number of maps for the vote
#[poise::command(slash_command, prefix_command, rename = "map_vote_count")]
async fn configure_map_vote_count(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.map_vote_count = new_value;
        format!("Map vote count set to {}", new_value)
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!("Map vote count is currently {}", data_lock.map_vote_count)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays or sets time maps for the vote (0 for no timeout)
#[poise::command(slash_command, prefix_command, rename = "map_vote_time")]
async fn configure_map_vote_time(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.map_vote_time = new_value;
        format!("Map vote time set to {}", new_value)
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!("Map vote time is currently {}", data_lock.map_vote_time)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Displays or sets number of maps for the vote
#[poise::command(slash_command, prefix_command, rename = "maximum_queue_cost")]
async fn configure_maximum_queue_cost(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "New value"] new_value: Option<f32>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.maximum_queue_cost = new_value;
        format!("Max queue cost set to {}", new_value)
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!(
            "Max queue cost is currently {}",
            data_lock.maximum_queue_cost
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
    #[description = "Queue to change"] queue: String,
    #[description = "Post match channel"]
    #[channel_types("Voice")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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

/// Sets the channel to move members to after the end of the game
#[poise::command(slash_command, prefix_command, rename = "audit_channel")]
async fn configure_audit_channel(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "Audit channel"]
    #[channel_types("Text")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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
    #[description = "Queue to change"] queue: String,
    #[description = "Register role"]
    new_value: Option<serenity::RoleId>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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

/// Sets whether or not match chat messages are logged
#[poise::command(slash_command, prefix_command, rename = "log_chats")]
async fn configure_log_chats(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[description = "Log chats"] new_value: Option<bool>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let response = if let Some(new_value) = new_value {
        let mut data_lock = ctx.data().configuration.get_mut(&queue_uuid).unwrap();
        data_lock.log_chats = new_value;
        format!("Log chats changed to {}", new_value.to_string())
    } else {
        let data_lock = ctx.data().configuration.get(&queue_uuid).unwrap();
        format!("Log chats is {}", data_lock.log_chats)
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Configures roles that can see match channels of matches their not in
#[poise::command(slash_command, prefix_command, rename = "visability_override_roles")]
async fn configure_visability_override_roles(
    ctx: Context<'_>,
    #[description = "Queue to change"] queue: String,
    #[flag] remove: bool,
    #[description = "Override role"] channel: Option<serenity::RoleId>,
) -> Result<(), Error> {
    let response = {
        let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
        //ensure queue is part of server
        if !ctx
            .data()
            .guild_data
            .lock()
            .unwrap()
            .get(&ctx.guild_id().unwrap())
            .unwrap()
            .queues
            .contains(&queue_uuid)
        {
            ctx.send(
                CreateReply::default()
                    .content(format!(
                        "Queue id {} is not part of this server",
                        queue_uuid.0
                    ))
                    .ephemeral(true),
            )
            .await?;
        }
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
        "configure_team_size",
        "configure_queue_category",
        "configure_queue_channels",
        "configure_team_count",
        "configure_post_match_channel",
        "configure_maps",
        "configure_map_vote_count",
        "configure_map_vote_time",
        "configure_maximum_queue_cost",
        "configure_register_role",
        "configure_audit_channel",
        "configure_log_chats",
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
pub async fn create_queue(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid::new();
    ctx.data().configuration.insert(queue_uuid, QueueConfiguration::default());
    ctx.data().current_games.insert(queue_uuid, HashSet::default());
    ctx.data().is_matchmaking.insert(queue_uuid, Option::default());
    ctx.data().leaver_data.insert(queue_uuid, HashMap::default());
    ctx.data().message_edit_notify.insert(queue_uuid, Arc::new(Notify::new()));
    ctx.data().player_bans.insert(queue_uuid, HashMap::new());
    ctx.data().player_data.insert(queue_uuid, HashMap::new());
    ctx.data().queue_idx.insert(queue_uuid, 0);
    ctx.data().queued_players.insert(queue_uuid, HashSet::new());

    ctx.data().guild_data.lock().unwrap().entry(ctx.guild_id().unwrap()).or_default().queues.push(queue_uuid);
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
    #[description = "Queue to change"] queue: String,
    #[description = "New config"] new_config: String,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
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
    #[description = "Queue to change"] queue: String,
) -> Result<(), Error> {
    let queue_uuid: QueueUuid = QueueUuid(Uuid::parse_str(&queue.as_str())?);
    //ensure queue is part of server
    if !ctx
        .data()
        .guild_data
        .lock()
        .unwrap()
        .get(&ctx.guild_id().unwrap())
        .unwrap()
        .queues
        .contains(&queue_uuid)
    {
        ctx.send(
            CreateReply::default()
                .content(format!(
                    "Queue id {} is not part of this server",
                    queue_uuid.0
                ))
                .ephemeral(true),
        )
        .await?;
    }
    let config =
        serde_json::to_string_pretty(&ctx.data().configuration.get(&queue_uuid).unwrap().clone())?;
    let response = format!("Configuration: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}
