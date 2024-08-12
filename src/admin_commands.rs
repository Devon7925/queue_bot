use chrono::TimeDelta;
use itertools::Itertools;
use poise::{
    serenity_prelude::{
        self as serenity, CreateActionRow, CreateAllowedMentions, CreateButton, CreateMessage,
        EditMember, Mentionable, UserId,
    },
    CreateReply,
};

use crate::{
    apply_match_results, log_match_results, update_bans, BanData, Context, DerivedPlayerData,
    Error, MatchResult,
};

#[poise::command(prefix_command, required_permissions = "MANAGE_CHANNELS")]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
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
    let ban_text = get_ban_text(&player, &ban_data);
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
        ban_text.clone()
    };
    let audit_channel = ctx.data().configuration.lock().unwrap().audit_channel;
    if let Some(audit_log) = audit_channel {
        audit_log
            .send_message(
                ctx.http(),
                CreateMessage::new()
                    .content(ban_text)
                    .allowed_mentions(CreateAllowedMentions::new().all_users(false)),
            )
            .await?;
    }
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
        let audit_channel = ctx.data().configuration.lock().unwrap().audit_channel;
        if let Some(audit_log) = audit_channel {
            audit_log
                .send_message(
                    ctx.http(),
                    CreateMessage::new()
                        .content(format!("Unbanned {}.", player.mention()))
                        .allowed_mentions(CreateAllowedMentions::new().all_users(false)),
                )
                .await?;
        }
        format!("Unbanned {}.", player.mention())
    } else {
        format!("{} was not banned.", player.mention())
    };
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Lists players banned from queueing
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "BAN_MEMBERS"
)]
pub async fn list_bans(ctx: Context<'_>) -> Result<(), Error> {
    update_bans(ctx.data().clone());
    let ban_data = ctx
        .data()
        .player_bans
        .lock()
        .unwrap()
        .iter()
        .map(|(id, ban_data)| get_ban_text(id, ban_data))
        .join("\n");

    let response = format!("# Player Bans\n{}", ban_data);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

fn get_ban_text(id: &UserId, ban_data: &BanData) -> String {
    let mut ban = format!("{} banned", id.mention());
    if let Some(reason) = ban_data.reason.clone() {
        ban += format!(" for {}", reason).as_str();
    }
    if let Some(end_time) = ban_data.end_time {
        ban += format!(" until <t:{}:f>", end_time.timestamp()).as_str();
    }
    ban
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
pub async fn manage_player(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Lists players who've left games
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "BAN_MEMBERS"
)]
pub async fn list_leavers(ctx: Context<'_>) -> Result<(), Error> {
    let leave_data = ctx
        .data()
        .leaver_data
        .lock()
        .unwrap()
        .iter()
        .map(|(id, count)| format!("{} left {} times", id.mention(), count))
        .join("\n");

    let response = format!("# Player Leave Counts\n{}", leave_data);
    ctx.send(CreateReply::default().content(response).ephemeral(true))
        .await?;
    Ok(())
}

/// Forces the outcome of a game
#[poise::command(slash_command, prefix_command, rename = "cancel")]
async fn force_outcome_cancel(ctx: Context<'_>) -> Result<(), Error> {
    force_result(ctx, MatchResult::Cancel).await
}

/// Forces the outcome of a game
#[poise::command(slash_command, prefix_command, rename = "draw")]
async fn force_outcome_draw(ctx: Context<'_>) -> Result<(), Error> {
    force_result(ctx, MatchResult::Tie).await
}

/// Forces the outcome of a game
#[poise::command(slash_command, prefix_command, rename = "team")]
async fn force_outcome_team(ctx: Context<'_>, #[min = 1] team_idx: u32) -> Result<(), Error> {
    force_result(ctx, MatchResult::Team(team_idx - 1)).await
}

/// Forces the outcome of a game
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "BAN_MEMBERS",
    subcommands("force_outcome_cancel", "force_outcome_draw", "force_outcome_team")
)]
pub async fn force_outcome(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn force_result(ctx: Context<'_>, result: MatchResult) -> Result<(), Error> {
    let match_number = {
        let match_channels = ctx.data().match_channels.lock().unwrap();
        match_channels.get(&ctx.channel_id()).cloned()
    };
    let Some(match_number) = match_number else {
        ctx.send(
            CreateReply::default()
                .content("Not in match: cannot force outcome.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };
    let post_match_channel = ctx
        .data()
        .configuration
        .lock()
        .unwrap()
        .post_match_channel
        .clone();
    let (channels, players) = {
        let match_data = ctx.data().match_data.lock().unwrap();
        let match_data = match_data.get(&match_number).unwrap();
        log_match_results(ctx.data().clone(), &result, &match_data, match_number);
        (match_data.channels.clone(), match_data.members.clone())
    };

    apply_match_results(ctx.data().clone(), result, &players);

    let guild_id = ctx.guild_id().unwrap();
    if let Some(post_match_channel) = post_match_channel {
        for player in players.iter().flat_map(|t| t) {
            ctx.data().in_game_players.lock().unwrap().remove(player);
            ctx.http()
                .get_member(guild_id, *player)
                .await?
                .edit(
                    ctx.http(),
                    EditMember::new().voice_channel(post_match_channel),
                )
                .await
                .ok();
        }
    }
    for channel in channels {
        ctx.data().match_channels.lock().unwrap().remove(&channel);
        ctx.http().delete_channel(channel, None).await?;
    }
    {
        let mut match_data = ctx.data().match_data.lock().unwrap();
        match_data.remove(&match_number);
    }
    Ok(())
}

/// Displays or sets number of maps for the vote
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "MANAGE_CHANNELS"
)]
pub async fn create_queue_message(ctx: Context<'_>) -> Result<(), Error> {
    let msg = ctx
        .send(
            CreateReply::default()
                .content("## Matchmaking queue")
                .components(vec![CreateActionRow::Buttons(vec![
                    CreateButton::new("queue")
                        .label("Join Queue")
                        .style(serenity::ButtonStyle::Primary),
                    CreateButton::new("leave_queue")
                        .label("Leave Queue")
                        .style(serenity::ButtonStyle::Danger),
                    CreateButton::new("status")
                        .label("Status")
                        .style(serenity::ButtonStyle::Secondary),
                ])])
                .ephemeral(false),
        )
        .await?
        .into_message()
        .await?
        .id;
    ctx.data()
        .configuration
        .lock()
        .unwrap()
        .queue_messages
        .push((ctx.channel_id(), msg));

    Ok(())
}
