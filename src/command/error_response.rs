use rootcause::{IntoReportCollection, Report, markers::SendSync, prelude::ResultExt};

use crate::CommandContext;

// pub trait LogError<T> {
//     async fn log_error(self, ctx: Context<'_>, msg: &str) -> Result<T, Report>;
// }

// impl<T, E: std::error::Error + IntoReportCollection<SendSync>> LogError<T> for Result<T, E> {
//     async fn log_error(self, ctx: Context<'_>, msg: &str) -> Result<T, Report> {
//         match self.context(msg) {
//             Ok(t) => Ok(t),
//             Err(e) => Err({
//                 let a = ctx
//                     .send(
//                         poise::CreateReply::default()
//                             .content(format!("{e}"))
//                             .ephemeral(true)
//                             .reply(true),
//                     )
//                     .await
//                     .context("Unable to log error message to author");
//             }),
//         }
//     }
// }

// impl<TOk> LogError for Result<TOk, Report> {
//     // async fn log_error<T>(self, ctx: Context<'_>) -> Result<T, Box<dyn std::error::Error>> {
//     //     if self.is_err() {
//     //         return ctx
//     //             .send(
//     //                 poise::CreateReply::default()
//     //                     .content(format!("{err}"))
//     //                     .ephemeral(true)
//     //                     .reply(true),
//     //             )
//     //             .await
//     //             .context("Unable to log error message to author");
//     //     }

//     //     self
//     // }
//     // async fn log_error(self, ctx: Context<'_>) -> Self {
//     //     match self {
//     //         Err(err) => {
//     //                 .is_err()
//     //             {
//     //                 self.context("Unable to log error message to author")
//     //             } else {
//     //                 self
//     //             }
//     //             // .context("Unable to log error message to author")
//     //         }
//     //         ok => ok,
//     //     }
//     //     // if self.is_err() {
//     //     // }
//     // }
// }

pub trait ResponseWithError<Return> {
    async fn respond_error(self, ctx: CommandContext<'_>, msg: &str) -> Return;
}

impl<T: Send, E: Send> ResponseWithError<Result<T, E>> for Result<T, E> {
    async fn respond_error(self, ctx: CommandContext<'_>, msg: &str) -> Result<T, E> {
        if let Err(_) = self {
            ctx.send(
                poise::CreateReply::default()
                    .content(msg)
                    .ephemeral(true)
                    .reply(true),
            )
            .await;
        }
        self
    }
}

impl<T> ResponseWithError<Option<T>> for Option<T> {
    async fn respond_error(self, ctx: CommandContext<'_>, msg: &str) -> Option<T> {
        if let None = self {
            ctx.send(
                poise::CreateReply::default()
                    .content(msg)
                    .ephemeral(true)
                    .reply(true),
            )
            .await;
        }
        self
    }
}

// async fn help_me<T, E>(a: Result<T, E>, ctx: Context<'_>, msg: &str) -> () {
//     if let Err(err) = a {
//         let context = ctx
//             .send(
//                 poise::CreateReply::default()
//                     .content(format!("{err}"))
//                     .ephemeral(true)
//                     .reply(true),
//             )
//             .await
//             .context("Unable to log failure to message author");
//         }
//     }

//     todo!()
// }
