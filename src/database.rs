use async_sqlite::{
    Client,
    rusqlite::{Connection, Error as SqliteError, OptionalExtension},
};
use jiff::Timestamp;
use rootcause::prelude::*;
use serenity::all::{ChannelId, UserId};

/// Whether a user is attending a hangout.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Attending {
    /// The user is registered for this hangout.
    Already,
    /// The user is newly interested in this hangout.
    New,
}

/// Checks if [`Connection`] is a hangout_channel.
fn is_hangout_channel(con: &Connection, channel: ChannelId) -> Result<bool, SqliteError> {
    Ok(con
        .query_one(
            "SELECT 1 FROM hangout_channel WHERE channel_id = ?1",
            [channel.get().cast_signed()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Gets the maximum attendees for a hangout.
fn get_maximum(con: &Connection, channel: ChannelId) -> Result<u8, SqliteError> {
    Ok(con.query_one(
        "SELECT max_attendees FROM hangout_channel WHERE channel_id = ?1",
        [channel.get().cast_signed()],
        |row| row.get::<usize, u8>(0),
    )?)
}

/// Creates the tables in the database if it does not exist.
pub async fn setup_db(db: &Client) -> Result<(), Report> {
    db.conn(|con| {
        con.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS hangout_channel (
    channel_id INT PRIMARY KEY,
    max_attendees INT NOT NULL
);

CREATE TABLE IF NOT EXISTS attendee (
    user_id INT NOT NULL,
    hangout_id INT NOT NULL,
    interest_time INT NOT NULL CHECK (interest_time >= 0),
    FOREIGN KEY (hangout_id) REFERENCES hangout_channel(channel_id) ON DELETE CASCADE,
    UNIQUE (user_id, hangout_id)
);
        "#,
        )
    })
    .await
    .context("Failed to setup db")?;

    Ok(())
}

/// Removes a hangout and all interested users from the database.
pub async fn remove_hangout(db: &Client, channel: ChannelId) -> Result<(), Report> {
    db.conn(move |con| {
        con.execute(
            "DELETE FROM hangout_channel WHERE channel_id = ?1",
            [channel.get().cast_signed()],
        )
    })
    .await
    .context("Unable to delete hangout")?;
    Ok(())
}

/// Creates or updates a hangout with the maximum attendees for a hangout.
pub async fn set_channel_max(db: &Client, channel: ChannelId, max: u8) -> Result<(), Report> {
    db.conn_mut(move |con| {
        let trans = con.transaction()?;

        let exists = trans
            .query_one(
                "SELECT 1 FROM hangout_channel WHERE channel_id = ?1",
                [channel.get().cast_signed()],
                |row| row.get::<usize, bool>(0),
            )
            .optional()?
            .is_some();

        if exists {
            trans.execute(
                "UPDATE hangout_channel SET max_attendees = ?2 WHERE channel_id = ?1",
                (channel.get().cast_signed(), max),
            )?;
        } else {
            trans.execute(
                "INSERT INTO hangout_channel VALUES (?1, ?2)",
                (channel.get().cast_signed(), max),
            )?;
        }

        trans.commit()?;
        Ok(())
    })
    .await
    .context("Cannot set maximum attendees")?;

    Ok(())
}

/// The maximum channel attendees, or [`None`] if this channel is not a hangout channel.
pub async fn get_channel_max(db: &Client, channel: ChannelId) -> Result<Option<u8>, Report> {
    Ok(db
        .conn(move |con| {
            con.query_one(
                "SELECT max_attendees FROM hangout_channel WHERE channel_id = ?1",
                [channel.get().cast_signed()],
                |row| row.get::<&str, u8>("max_attendees"),
            )
            .optional()
        })
        .await
        .context("Cannot get maximum attendees")?)
}

/// Returns [`None`] if this channel is not a hangout channel.
pub async fn set_interested(
    db: &Client,
    channel: ChannelId,
    user: UserId,
    at: Timestamp,
) -> Result<Option<Attending>, Report> {
    Ok(db
        .conn(move |con| {
            if !is_hangout_channel(con, channel)? {
                return Ok(None);
            }

            Ok(Some(con.execute(
                "INSERT OR IGNORE INTO attendee VALUES (?1, ?2, ?3)",
                (user.get().cast_signed(), channel.get().cast_signed(), at),
            )?))
        })
        .await
        .context("Unable to assign set user as attending")?
        .map(|is_new| {
            if is_new == 0 {
                Attending::Already
            } else {
                Attending::New
            }
        }))
}

/// Returns the total number of users that are interested or [`None`] if the channel is not a hangout channel.
pub async fn get_interested(db: &Client, channel: ChannelId) -> Result<Option<u8>, Report> {
    Ok(db
        .conn(move |con| {
            if !is_hangout_channel(con, channel)? {
                return Ok(None);
            }

            Ok(Some(con.query_one(
                "SELECT COUNT(1) FROM attendee WHERE hangout_id = ?1",
                [channel.get().cast_signed()],
                |row| row.get::<usize, u8>(0),
            )?))
        })
        .await
        .context("Cannot count attendees for a hangout")?)
}

/// Optionally returns the user to take their place if there is a queue.
pub async fn remove_interested(
    db: &Client,
    channel: ChannelId,
    user: UserId,
) -> Result<Option<UserId>, Report> {
    Ok(db
        .conn_mut(move |con| {
            let trans = con.transaction()?;
            trans.execute(
                "DELETE FROM attendee WHERE user_id = ?1 AND hangout_id = ?2",
                [user.get().cast_signed(), channel.get().cast_signed()],
            )?;

            // Offset cannot be an inline query, so has to be provided as a parameter.
            let offset = trans.query_one(
                "SELECT max_attendees FROM hangout_channel WHERE channel_id = ?1",
                [channel.get().cast_signed()],
                |row| row.get::<usize, u8>(0),
            )?;

            let next_attendee = trans
                .query_one(
                    r#"SELECT user_id FROM attendee
                JOIN hangout_channel ON channel_id = hangout_id
                WHERE hangout_id = ?1 AND (SELECT COUNT(*) FROM attendee WHERE hangout_id = ?1) > max_attendees
                ORDER BY interest_time ASC LIMIT 1, ?2"#,
                    (channel.get().cast_signed(), offset),
                    |row| row.get::<usize, i64>(0),
                ).optional()?
                .map(i64::cast_unsigned)
                .map(UserId::new);

            trans.commit()?;
            Ok(next_attendee)
        })
        .await
        .context("Cannot remove attendee")?)
}

/// The users attending a hangout, or [`None`] if the channel is not a hangout.
///
/// Element 1 of the tuple is the attending users.
/// Element 2 is the interested users.
pub async fn hangout_status(
    db: &Client,
    channel: ChannelId,
) -> Result<Option<(Vec<UserId>, Vec<UserId>)>, Report> {
    Ok(db
        .conn_mut(move |con| {
            let trans = con.transaction()?;

            if !is_hangout_channel(&trans, channel)? {
                return Ok(None);
            }

            let maximum = get_maximum(&trans, channel)?;

            let mut users = trans
                .prepare(
                    "SELECT user_id FROM attendee WHERE hangout_id = ?1 ORDER BY interest_time ASC",
                )?
                .query_map([channel.get().cast_signed()], |row| {
                    row.get::<usize, i64>(0)
                })?
                .map(|user| user.map(|user| UserId::new(user.cast_unsigned())))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter();

            Ok(Some((
                users.by_ref().take(maximum.into()).collect::<Vec<_>>(),
                users.collect::<Vec<_>>(),
            )))
        })
        .await
        .context("Unable to get attending users.")?)
}

// pub async fn setup_guild(
//     db: &Client,
//     guild_id: GuildId,
//     hangout_category: ChannelId,
//     archive_category: ChannelId,
// ) -> Result<(), Report> {
//     Ok(())
// }

// pub async fn get_hangout(db: &Client, guild_id: GuildId) -> Result<ChannelId, Report> {
//     todo!()
// }

// pub async fn new_hangout(
//     db: &Client,
//     guild_id: GuildId,
//     name: &str,
//     creator: (),
//     max_attendees: Option<u8>,
// ) {
//     todo!()
// }

#[cfg(test)]
mod tests {
    use async_sqlite::ClientBuilder;
    use std::assert_matches;

    use super::*;

    async fn setup() -> Client {
        let db = ClientBuilder::new()
            .open()
            .await
            .expect("Able to create sqlite in memory database,");

        std::assert_matches!(setup_db(&db).await, Ok(()));
        db
    }

    #[tokio::test]
    async fn setup_testing() {
        setup().await;
    }

    #[tokio::test]
    async fn new_channel_max() {
        let channel_id = ChannelId::new(1);
        let max = 2u8;

        let db = setup().await;
        set_channel_max(&db, channel_id, max)
            .await
            .expect("Able to set channel max");

        let conn = db
            .conn(|con| {
                con.query_row("SELECT * FROM hangout_channel", [], |row| {
                    Ok((row.get::<usize, i64>(0)?, row.get::<usize, u8>(1)?))
                })
            })
            .await
            .expect("Able to query inserted row");

        assert_eq!(channel_id, ChannelId::new(conn.0.cast_unsigned()));
        assert_eq!(conn.1, max);
    }

    #[tokio::test]
    async fn overwrite_channel_max() {
        let channel_id = ChannelId::new(1);
        let max = 2u8;

        let db = setup().await;
        set_channel_max(&db, channel_id, 3)
            .await
            .expect("Able to set channel max");

        let db = setup().await;
        set_channel_max(&db, channel_id, max)
            .await
            .expect("Able to overwrite channel max");

        let conn = db
            .conn(|con| {
                con.query_row("SELECT * FROM hangout_channel", [], |row| {
                    Ok((row.get::<usize, i64>(0)?, row.get::<usize, u8>(1)?))
                })
            })
            .await
            .expect("Able to query inserted row");

        assert_eq!(channel_id, ChannelId::new(conn.0.cast_unsigned()));
        assert_eq!(conn.1, max);
    }

    #[tokio::test]
    async fn getting_channel_max() {
        let channel_id = ChannelId::new(1);
        let max = 2u8;

        let db = setup().await;
        set_channel_max(&db, channel_id, max)
            .await
            .expect("Able to set channel max");

        let get_channel_max = get_channel_max(&db, channel_id)
            .await
            .expect("Able to get max");

        assert_eq!(Some(max), get_channel_max);
    }

    #[tokio::test]
    async fn get_unset_max() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        let get_channel_max = get_channel_max(&db, channel_id)
            .await
            .expect("Able to get max");

        assert_eq!(None, get_channel_max);
    }

    #[tokio::test]
    async fn interested_no_hangout() {
        let db = setup().await;

        let set_interested = set_interested(&db, 1u64.into(), 1u64.into(), Timestamp::default())
            .await
            .unwrap();

        assert_eq!(set_interested, None);
    }

    #[tokio::test]
    async fn interested() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 255)
            .await
            .expect("Able to set channel max");

        let set_interested = set_interested(&db, channel_id, 1u64.into(), Timestamp::default())
            .await
            .unwrap();

        assert_eq!(set_interested, Some(Attending::New));
    }

    #[tokio::test]
    async fn interested_within_cap() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 255)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let interested = get_interested(&db, channel_id).await.unwrap();
        assert_matches!(interested, Some(3));
    }

    #[tokio::test]
    async fn interested_over_cap() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 2)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let interested = get_interested(&db, channel_id).await.unwrap();
        assert_matches!(interested, Some(3));
    }

    #[tokio::test]
    async fn remove_interest_no_queue() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 255)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let next = remove_interested(&db, channel_id, 1.into()).await.unwrap();
        std::assert_matches!(next, None);
    }

    #[tokio::test]
    async fn remove_interest_queue() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 1)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let next = remove_interested(&db, channel_id, 1.into()).await.unwrap();
        assert_matches!(next, Some(user) if user.get() == 3);
    }

    #[tokio::test]
    async fn remove_interest_one_queue() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 2)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let next = remove_interested(&db, channel_id, 1.into()).await.unwrap();
        assert_matches!(next, None);
    }

    #[tokio::test]
    async fn hangout_remove() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 2)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        assert_matches!(
            remove_hangout(&db, channel_id).await,
            Ok(()),
            "Able to delete hangout and attendees."
        );

        db.conn(|con| {
            let hangouts = con
                .query_one("SELECT COUNT(*) FROM hangout_channel", [], |row| {
                    row.get::<usize, i32>(0)
                })
                .unwrap();

            assert_eq!(hangouts, 0, "The only present hangout was deleted.");

            let attendees = con
                .query_one("SELECT COUNT(*) FROM attendee", [], |row| {
                    row.get::<usize, i32>(0)
                })
                .unwrap();

            assert_eq!(attendees, 0, "All attendees are deleted");

            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn hangout_remove_one() {
        let channel_one = ChannelId::new(1);
        let channel_two = ChannelId::new(2);

        let db = setup().await;

        set_channel_max(&db, channel_one, 2)
            .await
            .expect("Able to set channel max");

        set_channel_max(&db, channel_two, 2)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_one, id.into(), Timestamp::default())
                .await
                .unwrap();

            set_interested(&db, channel_two, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        set_interested(&db, channel_two, 6.into(), Timestamp::default())
            .await
            .unwrap();

        assert_matches!(
            remove_hangout(&db, channel_one).await,
            Ok(()),
            "Able to delete hangout and attendees."
        );

        db.conn(|con| {
            let remaining_channel = con
                .query_one("SELECT channel_id FROM hangout_channel", [], |row| {
                    row.get::<usize, i32>(0)
                })
                .unwrap();

            assert_eq!(
                remaining_channel, 2,
                "Only the specified channel was deleted."
            );

            let attendees = con
                .query_one("SELECT COUNT(*) FROM attendee", [], |row| {
                    row.get::<usize, i32>(0)
                })
                .unwrap();

            // Take the easy way out since hangouts have different number of attendees.
            assert_eq!(attendees, 4, "All attendees for channel 1 were deleted.");

            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn status() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 10)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let (attending, interested) = hangout_status(&db, channel_id).await.unwrap().unwrap();
        assert_eq!(attending, vec![1, 2, 3]);
        assert!(interested.is_empty())
    }

    #[tokio::test]
    async fn status_with_interested() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 2)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        let (attending, interested) = hangout_status(&db, channel_id).await.unwrap().unwrap();
        assert_eq!(attending, vec![1, 2]);
        assert_eq!(interested, vec![3]);
    }

    #[tokio::test]
    async fn max_update_users_retained() {
        let channel_id = ChannelId::new(1);

        let db = setup().await;

        set_channel_max(&db, channel_id, 2)
            .await
            .expect("Able to set channel max");

        for id in 1..4 {
            set_interested(&db, channel_id, id.into(), Timestamp::default())
                .await
                .unwrap();
        }

        set_channel_max(&db, channel_id, 7)
            .await
            .expect("Able to set channel max");

        let (attending, _) = hangout_status(&db, channel_id).await.unwrap().unwrap();
        assert_eq!(attending, vec![1, 2, 3]);
    }
}
