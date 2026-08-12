use crate::{
    CommandContext, CommandError,
    command::ephemeral_reply,
    database::{
        get_channel_max, get_interested, hangout_status, remove_hangout, remove_interested,
        set_channel_max, set_interested,
    },
};
use jiff::Timestamp;
use poise::CreateReply;
use rootcause::prelude::*;
use serenity::all::{CreateAllowedMentions, CreateEmbed, MessageBuilder};

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

/// Shows help for commands.
#[poise::command(slash_command, prefix_command)]
pub async fn help(
    ctx: CommandContext<'_>,
    #[description = "Optionally, a specific command to view help of."] specific_command: Option<
        String,
    >,
) -> Result<(), CommandError> {
    poise::builtins::pretty_help(
        ctx,
        specific_command.as_deref(),
        poise::builtins::PrettyHelpConfiguration {
            ..Default::default()
        },
    )
    .await
    .context("Unable to send help command.")?;

    Ok(())
}

/// Explains how to use the bot.
#[poise::command(slash_command, prefix_command)]
pub async fn guide(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    let guide_text = "\
This discord bot helps manage who is attending hangouts in a first come first serve basis, \
including when users drop-out of a hangout.

- A hangout is in a specific channel, with a maximum number of attendees (use 255 if there is no \
maximum).
- People can register to attend the hangout via the discord bot.
- If more people are interested than available slots a first come first serve queue is created \
based on the time interest was expressed.
- If someone leaves (either during or before the event) a message will be sent to the first person \
waiting in the queue (if there are any).

See the **help** command for information on how to use commands or a specific command.

If you have any further questions, ask them in the server."
        .as_bytes()
        .chunks(1998)
        .map(str::from_utf8)
        .collect::<Result<Vec<&str>, _>>()
        .expect("Unable to split up guide message");

    for text in guide_text {
        ctx.send(ephemeral_reply(text))
            .await
            .context("Unable to send message.")?;
    }

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
///
/// Only relivant for channels that will house multiple hangouts sequentially.
#[poise::command(slash_command, prefix_command)]
pub async fn event_over(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    remove_hangout(&ctx.data().db, ctx.channel_id())
        .await
        .context("Cannot remove hangout and attendees.")?;

    ctx.say("This channel is no longer a hangout.")
        .await
        .context("Cannot send hangout remove message")?;

    Ok(())
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

/// Displays the current attending and interested users along with the maximum spaces.
#[poise::command(slash_command, prefix_command)]
pub async fn status(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    let maximum = get_channel_max(&ctx.data().db, ctx.channel_id())
        .await
        .context("Cannot get hangout channel maximum")?;

    let (Some((attending, interested)), Some(max)) = (
        hangout_status(&ctx.data().db, ctx.channel_id()).await?,
        maximum,
    ) else {
        ctx.send(ephemeral_reply("This channel is not a hangout."))
            .await
            .context("Unable to send reply")?;
        return Ok(());
    };

    let amount_attending = attending.len();

    let list_user = |mut message: MessageBuilder, user| {
        message.push("- ").user(user).push("\n");
        message
    };

    let attending_message = attending
        .into_iter()
        .fold(MessageBuilder::new(), list_user)
        .build();
    let interested_message = interested
        .into_iter()
        .fold(MessageBuilder::new(), list_user)
        .build();

    ctx.send(
        CreateReply::default()
            .allowed_mentions(CreateAllowedMentions::new().empty_users())
            .embed(
                CreateEmbed::new()
                    .title("Status")
                    .field("Attending", attending_message, false)
                    .field("Interested", interested_message, false)
                    .field("Spaces", format!("{amount_attending}/{max}",), false),
            )
            .reply(true)
            .ephemeral(true),
    )
    .await
    .context("Unable to send hangout status to user")?;

    Ok(())
}
