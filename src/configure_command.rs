use itertools::Itertools;
use poise::{serenity_prelude::{self as serenity, Mentionable}, CreateReply};

use crate::{Context, Error, QueueConfiguration};

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

/// Displays or sets time maps for the vote (0 for no timeout)
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
}

/// Sets the channel to move members to after the end of the game
#[poise::command(slash_command, prefix_command, rename = "audit_channel")]
async fn configure_audit_channel(
    ctx: Context<'_>,
    #[description = "Audit channel"]
    #[channel_types("Text")]
    new_value: Option<serenity::Channel>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.audit_channel = Some(new_value.id());
            format!("Audit channel changed to {}", new_value.to_string())
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
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
}

/// Sets whether or not match chat messages are logged
#[poise::command(slash_command, prefix_command, rename = "log_chats")]
async fn configure_log_chats(
    ctx: Context<'_>,
    #[description = "Log chats"] new_value: Option<bool>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.log_chats = new_value;
            format!("Log chats changed to {}", new_value.to_string())
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!("Log chats is {}", data_lock.log_chats)
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
}

/// Configures roles that can see match channels of matches their not in
#[poise::command(slash_command, prefix_command, rename = "visability_override_roles")]
async fn configure_visability_override_roles(
    ctx: Context<'_>,
    #[description = "New override role"] new_value: Option<serenity::RoleId>,
) -> Result<(), Error> {
    if let Some(new_value) = new_value {
        let response = {
            let mut data_lock = ctx.data().configuration.lock().unwrap();
            data_lock.visability_override_roles.push(new_value);
            format!("{} added as override role", new_value.to_string())
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    } else {
        let response = {
            let data_lock = ctx.data().configuration.lock().unwrap();
            format!(
                "Override roles are {}",
                data_lock
                    .visability_override_roles
                    .iter()
                    .map(|c| format!("{}", c.mention()))
                    .join(", ")
            )
        };
        ctx.send(CreateReply::default().content(response).ephemeral(true))
            .await?;
        Ok(())
    }
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
        "configure_audit_channel",
        "configure_log_chats",
        "configure_visability_override_roles",
    )
)]
pub async fn configure(_: Context<'_>) -> Result<(), Error> {
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
) -> Result<(), Error> {
    let new_config: QueueConfiguration = serde_json::from_str(&new_config.as_str())?;
    *ctx.data().configuration.lock().unwrap() = new_config;
    let config = serde_json::to_string_pretty(ctx.data())?;
    let response = format!("Configuration set to: ```json\n{}\n```", config);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}