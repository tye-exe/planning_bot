use crate::{
    AppSate, CommandContext, CommandError,
    command::ephemeral_reply,
    database::{
        get_channel_max, get_interested, remove_hangout, remove_interested, set_channel_max,
        set_interested,
    },
};
use jiff::Timestamp;
use poise::CreateReply;
use rootcause::prelude::*;
use serenity::all::MessageBuilder;

/// Menu for registering commands in the guild or globally.
///
/// Global commands can only be modified by bot owner.
#[poise::command(slash_command, prefix_command)]
pub async fn command_registration(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    poise::builtins::register_application_commands_buttons(ctx)
        .await
        .context("Unable to register application commands.")?;

    Ok(())
}

/// Set a hangout in the current channel.
#[poise::command(slash_command, prefix_command)]
pub async fn hangout(
    ctx: CommandContext<'_>,
    #[description = "Maximum attendiees"] attendeiees: u8,
) -> Result<(), CommandError> {
    set_channel_max(&ctx.data().db, ctx.channel_id(), attendeiees)
        .await
        .context("Unable to set maximum attendies")?;

    ctx.say(format!(
        "A maximum of {attendeiees} people can attend this event."
    ))
    .await
    .context("Cannot mention number of attendeiees.")?;

    Ok(())
}

/// Remove the hangout in the current channel the and all interested users.
#[poise::command(slash_command, prefix_command)]
pub async fn event_over(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    Ok(remove_hangout(&ctx.data().db, ctx.channel_id())
        .await
        .context("Cannot remove hangout and attendees.")?)
}

/// Register your interest for the current hangout.
///
/// If there is an open space you will be marked as attending the hangout.
///
/// If the hangout is already full you will be put in a queue in case anyone drops out and you will
/// take there space (first come first serve).
#[poise::command(slash_command, prefix_command)]
pub async fn interested(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    set_interested(
        &ctx.data().db,
        ctx.channel_id(),
        ctx.author().id,
        Timestamp::now(),
    )
    .await?;

    let (interested, max_spaces) = match (
        get_interested(&ctx.data().db, ctx.channel_id()).await?,
        get_channel_max(&ctx.data().db, ctx.channel_id()).await?,
    ) {
        (Some(interested), Some(max_spaces)) => (interested, max_spaces),
        (None, _) | (_, None) => {
            ctx.send(
                CreateReply::default()
                    .reply(true)
                    .ephemeral(true)
                    .content("This is not a hangout channel!"),
            )
            .await?;
            return Ok(());
        }
    };

    let content = MessageBuilder::new()
        .mention(&ctx.author().id)
        .push(" is interested. ")
        .push(if interested > max_spaces {
            format!(
                "All the spaces are currently full. \
                    There are {} people waiting for a space before you",
                (interested - 1) - max_spaces
            )
        } else {
            format!("There are {} spaces left", max_spaces - interested)
        })
        .build();

    ctx.say(content)
        .await
        .context("Cannot state number of attendeiees.")?;

    Ok(())
}

/// Remove interest the current hangout (whether before or during if others want to attend).
///
/// If there are people who want to join but cannot due to an attendee limit, one of them will
/// take your space.
#[poise::command(slash_command, prefix_command)]
pub async fn leave(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    let new_space = remove_interested(&ctx.data().db, ctx.channel_id(), ctx.author().id).await?;

    let content = MessageBuilder::new()
        .mention(&ctx.author().id)
        .push(" is not interested in the event. ")
        .push(if let Some(user) = new_space {
            MessageBuilder::new()
                .mention(&user)
                .push(" is now invited as this user is next in queue.")
                .build()
        } else {
            "There is no one else in the attendance queue.".to_owned()
        })
        .build();

    ctx.say(content)
        .await
        .context("Cannot state attendance changes.")?;

    Ok(())
}
