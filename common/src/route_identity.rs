#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteIdentity {
    key: u64,
    session: u64,
}

impl RouteIdentity {
    pub fn from_signed(key: i64, session: i64) -> Option<Self> {
        let key = u64::try_from(key).ok()?;
        let session = u64::try_from(session).ok()?;
        (key > 0 && session > 0).then_some(Self { key, session })
    }

    pub const fn key(self) -> u64 {
        self.key
    }

    pub const fn session(self) -> u64 {
        self.session
    }
}

#[cfg(test)]
mod tests {
    use super::RouteIdentity;

    #[test]
    fn signed_player_identity_becomes_positive_rpc_route() {
        let route = RouteIdentity::from_signed(42, 9001).unwrap();

        assert_eq!(route.key(), 42);
        assert_eq!(route.session(), 9001);
    }

    #[test]
    fn non_positive_signed_route_values_are_rejected() {
        assert!(RouteIdentity::from_signed(0, 1).is_none());
        assert!(RouteIdentity::from_signed(1, 0).is_none());
        assert!(RouteIdentity::from_signed(-1, 1).is_none());
        assert!(RouteIdentity::from_signed(1, -1).is_none());
    }
}
