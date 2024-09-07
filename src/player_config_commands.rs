use poise::CreateReply;

use crate::{Context, DerivedPlayerData, Error};

macro_rules! configure_player_variable {
    ($func_name:ident, $prop:ident, $rename:expr, $name:expr, $doc:expr) => {
#[doc=$doc]
#[poise::command(slash_command, rename=$rename)]
pub async fn $func_name(
    ctx: Context<'_>,
    #[description = "New value"]
    #[min = 0]
    new_value: Option<f32>,
    #[description = "Queue index"]
    #[min = 0]
    queue_idx: Option<u32>,
) -> Result<(), Error> {
    let queue_uuid = {
        let queues = ctx.data().guild_data.lock().unwrap().get(&ctx.guild_id().unwrap()).unwrap().queues.clone();
        if queues.len() == 0 {
            ctx.send(CreateReply::default().content(format!("No queues available.")).ephemeral(true))
                .await?;
            return Ok(())
        } else if let Some(queue_idx) = queue_idx {
            if let Some(queue) = queues.get(queue_idx as usize) {
                queue.clone()
            } else {
                ctx.send(CreateReply::default().content(format!("Invalid queue idx.")).ephemeral(true))
                    .await?;
                return Ok(())
            }
        }  else if queues.len() == 1 {
            queues.get(0).unwrap().clone()
        }else {
            ctx.send(CreateReply::default().content(format!("Multiple queues available: you must specify which queue you want to use")).ephemeral(true))
                .await?;
            return Ok(())
        }
    };
    let response = {
        let mut data_lock = ctx.data().player_data.get_mut(&queue_uuid).unwrap();
        let data_lock = data_lock.entry(ctx.author().id).or_insert(DerivedPlayerData::default());
        if let Some(new_value) = new_value {
            data_lock.player_queueing_config.$prop = Some(new_value);
            format!("{} set to {}", $name, new_value)
        } else {
            let default_value = ctx.data().configuration.get(&queue_uuid).unwrap().default_player_data.player_queueing_config.$prop;
            format!("{} is currently {}", $name, data_lock.player_queueing_config.$prop.unwrap_or(default_value))
        }
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}
    };
}

struct PlayerVariableModifiers;
impl PlayerVariableModifiers {
    configure_player_variable!(configure_player_cost_per_avg_mmr_differential, cost_per_avg_mmr_differential, "cost_per_avg_mmr_differential", "Average mmr difference cost", "Sets the cost for the difference in average player mmr between the teams above a certain threshold");
    configure_player_variable!(configure_player_acceptable_mmr_differential, acceptable_mmr_differential, "acceptable_mmr_differential", "Acceptable average mmr difference", "Sets the acceptable difference in average player mmr between the teams before cost starts increasing");
    configure_player_variable!(configure_player_cost_per_mmr_std_differential, cost_per_mmr_std_differential, "cost_per_mmr_std_differential", "Cost for difference in mmr variation", "Sets the cost for difference in mmr standard deviation between the teams above a certain threshold");
    configure_player_variable!(configure_player_acceptable_mmr_std_differential, acceptable_mmr_std_differential, "acceptable_mmr_std_differential", "Acceptable mmr variation difference", "Sets the acceptable difference in mmr std between the teams before cost starts increasing");
    configure_player_variable!(configure_player_cost_per_mmr_range, cost_per_mmr_range, "cost_per_mmr_range", "Cost for mmr range", "Sets the cost for the difference in mmr between the highest and lowest rated players");
    configure_player_variable!(configure_player_acceptable_mmr_range, acceptable_mmr_range, "acceptable_mmr_range", "Acceptable mmr range", "Sets acceptable difference in mmr between the highest and lowest rated players before adding cost");
    configure_player_variable!(configure_new_lobby_host_cost, new_lobby_host_cost, "new_lobby_host_cost", "Cost for new lobby host", "Sets cost for getting a different lobby host");
}

/// Displays your or another user's account creation date
#[poise::command(
    slash_command,
    prefix_command,
    subcommands(
        "PlayerVariableModifiers::configure_player_cost_per_avg_mmr_differential",
        "PlayerVariableModifiers::configure_player_acceptable_mmr_differential",
        "PlayerVariableModifiers::configure_player_cost_per_mmr_std_differential",
        "PlayerVariableModifiers::configure_player_acceptable_mmr_std_differential",
        "PlayerVariableModifiers::configure_player_cost_per_mmr_range",
        "PlayerVariableModifiers::configure_player_acceptable_mmr_range",
        "PlayerVariableModifiers::configure_new_lobby_host_cost",
    )
)]
pub async fn player_config(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}
