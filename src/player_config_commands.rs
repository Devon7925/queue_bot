use poise::CreateReply;

use crate::{Context, DerivedPlayerData, Error};
/// Sets the cost for the difference in average player mmr between the teams above a certain threshold
#[poise::command(slash_command, prefix_command, rename = "cost_per_avg_mmr_difference")]
async fn configure_player_cost_per_avg_mmr_differential(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<f32>,
) -> Result<(), Error> {
    let response = {
        let mut data_lock = ctx.data().player_data.lock().unwrap();
        let data_lock = data_lock.entry(ctx.author().id).or_insert(DerivedPlayerData::default());
        if let Some(new_value) = new_value {
            data_lock.player_queueing_config.cost_per_avg_mmr_differential = Some(new_value);
            format!("Average mmr difference cost set to {}", new_value)
        } else {
            let default_value = ctx.data().configuration.lock().unwrap().default_player_data.player_queueing_config.cost_per_avg_mmr_differential;
            format!("Average mmr difference cost is currently {}", data_lock.player_queueing_config.cost_per_avg_mmr_differential.unwrap_or(default_value))
        }
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Sets the acceptable difference in average player mmr between the teams before cost starts increasing
#[poise::command(slash_command, prefix_command, rename = "acceptable_mmr_difference")]
async fn configure_player_acceptable_mmr_differential(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<f32>,
) -> Result<(), Error> {
    let response = {
        let mut data_lock = ctx.data().player_data.lock().unwrap();
        let data_lock = data_lock.entry(ctx.author().id).or_insert(DerivedPlayerData::default());
        if let Some(new_value) = new_value {
            data_lock.player_queueing_config.acceptable_mmr_differential = Some(new_value);
            format!("Acceptable average mmr difference set to {}", new_value)
        } else {
            let default_value = ctx.data().configuration.lock().unwrap().default_player_data.player_queueing_config.acceptable_mmr_differential;
            format!("Acceptable average mmr difference is currently {}", data_lock.player_queueing_config.acceptable_mmr_differential.unwrap_or(default_value))
        }
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Sets the cost for the difference in mmr between the highest and lowest rated players
#[poise::command(slash_command, prefix_command, rename = "cost_per_mmr_range")]
async fn configure_player_cost_per_mmr_range(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<f32>,
) -> Result<(), Error> {
    let response = {
        let mut data_lock = ctx.data().player_data.lock().unwrap();
        let data_lock = data_lock.entry(ctx.author().id).or_insert(DerivedPlayerData::default());
        if let Some(new_value) = new_value {
            data_lock.player_queueing_config.cost_per_mmr_range = Some(new_value);
            format!("Cost for mmr range set to {}", new_value)
        } else {
            let default_value = ctx.data().configuration.lock().unwrap().default_player_data.player_queueing_config.cost_per_mmr_range;
            format!("Cost for mmr range is currently {}", data_lock.player_queueing_config.cost_per_mmr_range.unwrap_or(default_value))
        }
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Sets acceptable difference in mmr between the highest and lowest rated players before adding cost
#[poise::command(slash_command, prefix_command, rename = "acceptable_mmr_range")]
async fn configure_player_acceptable_mmr_range(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<f32>,
) -> Result<(), Error> {
    let response = {
        let mut data_lock = ctx.data().player_data.lock().unwrap();
        let data_lock = data_lock.entry(ctx.author().id).or_insert(DerivedPlayerData::default());
        if let Some(new_value) = new_value {
            data_lock.player_queueing_config.acceptable_mmr_range = Some(new_value);
            format!("Acceptable mmr range set to {}", new_value)
        } else {
            let default_value = ctx.data().configuration.lock().unwrap().default_player_data.player_queueing_config.acceptable_mmr_range;
            format!("Acceptable mmr range is currently {}", data_lock.player_queueing_config.acceptable_mmr_range.unwrap_or(default_value))
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
    subcommands(
        "configure_player_cost_per_avg_mmr_differential",
        "configure_player_acceptable_mmr_differential",
        "configure_player_cost_per_mmr_range",
        "configure_player_acceptable_mmr_range",
    )
)]
pub async fn player_config(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}