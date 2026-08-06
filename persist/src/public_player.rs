use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};

use xkk_protocol::pb;

pub struct PublicPlayer {
    gid: i64,
    data: RwLock<pb::PublicPlayerData>,
    dirty: AtomicBool,
}

impl PublicPlayer {
    pub fn new(gid: i64, mut data: pb::PublicPlayerData) -> Self {
        assert!(gid > 0, "Public player gid must be positive");
        data.gid = gid;
        let mail = data.mail.get_or_insert_default();
        mail.next_mail_id = mail.next_mail_id.max(1);
        Self {
            gid,
            data: RwLock::new(data),
            dirty: AtomicBool::new(false),
        }
    }

    pub fn empty(gid: i64) -> Self {
        Self::new(
            gid,
            pb::PublicPlayerData {
                gid,
                mail: Some(pb::MailData {
                    next_mail_id: 1,
                    mails: Vec::new(),
                }),
            },
        )
    }

    pub fn gid(&self) -> i64 {
        self.gid
    }

    pub fn read<R>(&self, read: impl FnOnce(&pb::PublicPlayerData) -> R) -> R {
        let data = self.data.read().expect("Public player data lock poisoned");
        read(&data)
    }

    pub fn update<R>(
        &self,
        update: impl FnOnce(&mut pb::PublicPlayerData) -> (R, bool),
        on_dirty: impl FnOnce(),
    ) -> R {
        let mut data = self.data.write().expect("Public player data lock poisoned");
        let (result, changed) = update(&mut data);
        if changed {
            self.dirty.store(true, Ordering::Release);
            on_dirty();
        }
        result
    }

    pub fn take_dirty_snapshot(&self) -> Option<pb::PublicPlayerData> {
        let data = self.data.write().expect("Public player data lock poisoned");
        self.dirty
            .swap(false, Ordering::AcqRel)
            .then(|| data.clone())
    }

    pub fn remove_registration_if_clean(&self, remove: impl FnOnce()) {
        let _data = self.data.write().expect("Public player data lock poisoned");
        if !self.dirty.load(Ordering::Acquire) {
            remove();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }
}

impl xlru::CacheValue for PublicPlayer {
    fn is_dirty(&self) -> bool {
        self.is_dirty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_document_becomes_a_clean_empty_public_player() {
        let player = PublicPlayer::empty(5);

        player.read(|data| {
            assert_eq!(data.gid, 5);
            let mail = data.mail.as_ref().unwrap();
            assert_eq!(mail.next_mail_id, 1);
            assert!(mail.mails.is_empty());
        });
        assert!(!player.is_dirty());
    }

    #[test]
    fn public_player_marks_changes_and_clears_snapshot_before_save() {
        let player = PublicPlayer::empty(7);
        let mut registered = false;

        let mail_id = player.update(
            |data| {
                let mail = data.mail.as_mut().unwrap();
                let id = mail.next_mail_id;
                mail.next_mail_id += 1;
                (id, true)
            },
            || registered = true,
        );

        assert_eq!(mail_id, 1);
        assert!(registered);
        assert!(player.is_dirty());
        let snapshot = player.take_dirty_snapshot().unwrap();
        assert_eq!(snapshot.mail.unwrap().next_mail_id, 2);
        assert!(!player.is_dirty());
        assert!(player.take_dirty_snapshot().is_none());
    }

    #[test]
    fn clean_registration_is_removed_while_holding_the_player_lock() {
        let player = PublicPlayer::empty(9);
        let mut removed = false;

        player.remove_registration_if_clean(|| removed = true);

        assert!(removed);
    }

    #[test]
    fn a_write_after_snapshot_keeps_the_dirty_registration() {
        let player = PublicPlayer::empty(11);
        player.update(|_| ((), true), || {});
        assert!(player.take_dirty_snapshot().is_some());

        player.update(|_| ((), true), || {});
        let mut removed = false;
        player.remove_registration_if_clean(|| removed = true);

        assert!(!removed);
        assert!(player.is_dirty());
    }
}
