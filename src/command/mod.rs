use poise::CreateReply;

/// Discord bot commands.
pub mod bot_command;

/// A [`CreateReply`] that is set to be ephemeral and a reply.
fn ephemeral_reply(content: impl Into<String>) -> CreateReply {
    CreateReply::default()
        .ephemeral(true)
        .reply(true)
        .content(content)
}
