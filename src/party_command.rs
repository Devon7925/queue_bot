use std::{collections::HashSet, sync::Arc};

use itertools::Itertools;
use poise::{
    serenity_prelude::{CacheHttp, CreateMessage, Mentionable, UserId},
    CreateReply,
};

use crate::{
    ButtonData, Context, Data, Error, GlobalPlayerData, GroupUuid, QueueGroup, QueueState,
};

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
    if let Some(failure_message) = match queue_state {
        QueueState::Queued(..) => Some(format!("Cannot invite players to party while in queue")),
        QueueState::InGame => Some(format!("Cannot invite players to party while in game")),
        QueueState::None => None,
    } {
        ctx.send(
            CreateReply::default()
                .content(failure_message)
                .ephemeral(true),
        )
        .await?;
        return Ok(());
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
                .button(ButtonData::JoinParty(party).get_button())
                .button(ButtonData::RejectParty(party).get_button()),
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

pub async fn leave_party(
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

/// Lists parties
#[poise::command(slash_command, prefix_command)]
pub async fn list_parties(ctx: Context<'_>) -> Result<(), Error> {
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
    subcommands("party_invite", "party_leave", "party_list")
)]
pub async fn party(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}
