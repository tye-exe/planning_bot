use crate::{
    AppSate, CommandContext, CommandError,
    database::{
        get_channel_max, get_interested, remove_hangout, remove_interested, set_channel_max,
        set_interested,
    },
};
use jiff::Timestamp;
use poise::CreateReply;
use rootcause::prelude::*;
use serenity::all::MessageBuilder;

pub mod error_response;

/// A [`CreateReply`] that is set to be ephemeral and a reply.
fn ephemeral_reply(content: impl Into<String>) -> CreateReply {
    CreateReply::default()
        .ephemeral(true)
        .reply(true)
        .content(content)
}

/// Register bot commands in the current guild.
///
/// This will take effect instantly, but will lead to duplicate commands in the long run due to
/// global command registration.
#[poise::command(slash_command, prefix_command)]
pub async fn guild_register(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.send(ephemeral_reply("Not in a guild"))
            .await
            .context("Cannot send reply")?;

        return Ok(());
    };

    poise::builtins::register_in_guild(ctx, &ctx.framework().options().commands, guild_id).await?;

    ctx.send(ephemeral_reply("Registered commands in guild"))
        .await
        .context("Cannot send reply")?;

    Ok(())
}

/// Unregister bot commands in the current guild.
#[poise::command(slash_command, prefix_command)]
pub async fn guild_unregister(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.send(ephemeral_reply("Not in a guild"))
            .await
            .context("Cannot send reply")?;

        return Ok(());
    };

    // Register an empty array of commands.
    poise::builtins::register_in_guild(
        ctx,
        &Vec::<poise::Command<AppSate, CommandError>>::new(),
        guild_id,
    )
    .await?;

    ctx.send(ephemeral_reply("Unregistered commands in guild"))
        .await
        .context("Cannot send reply")?;

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

// #[poise::command(slash_command, prefix_command)]
// pub async fn setup(
//     ctx: Context<'_>,
//     hangout_category: ChannelId,
//     archive_category: ChannelId,
// ) -> Result<(), Error> {
//     let guild_id = ctx.guild_id().context("Not in a guild")?;
//     setup_guild(&ctx.data().db, guild_id, hangout_category, archive_category).await?;
//     ctx.reply("Success")
//         .await
//         .context("Can send success message")?;
//     Ok(())
// }

// #[poise::command(slash_command, prefix_command)]
// pub async fn create_hangout(
//     ctx: Context<'_>,

//     #[description = "Name of the channel"] name: String,
//     #[description = "The maximum number of attendees, if applicable"] max_attendees: Option<u8>,
// ) -> Result<(), Error> {
//     let Some(guild_id) = ctx.guild_id().respond_error(ctx, "Not in guild").await else {
//         return Ok(());
//     };

//     let category = get_hangout(&ctx.data().db, guild_id)
//         .await
//         .respond_error(ctx, "You need to run the setup command first")
//         .await?;

//     // let guild = ctx.guild().unwrap();
//     let create_channel = ctx
//         .partial_guild()
//         .await
//         .respond_error(ctx, "Unable")
//         .await
//         .context("Unable")?
//         .create_channel(ctx, CreateChannel::new(name).category(category))
//         .await;
//     // .await?;

//     // create_channel
//     //     .respond_error(ctx, "Unable to create hangout")
//     //     .await?;
//     // category.cat

//     todo!()
// }
