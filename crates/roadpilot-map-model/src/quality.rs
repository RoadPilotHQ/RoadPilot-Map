//! Evidence level of a geometric value.

/// How well the game data supports a geometric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Quality {
    /// Established by the game data itself.
    Confirmed,
    /// Produced by a model that is only empirically supported, or from
    /// assumptions supplied by the caller.
    Estimated,
    /// Not known. No value is produced and none may be assumed.
    Unavailable,
}

impl Quality {
    /// The name used in the map package: `confirmed`, `estimated` or
    /// `unavailable`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Estimated => "estimated",
            Self::Unavailable => "unavailable",
        }
    }

    /// Parses the name written by [`Quality::as_str`].
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "confirmed" => Some(Self::Confirmed),
            "estimated" => Some(Self::Estimated),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }

    /// `true` if a value exists at all (confirmed or estimated).
    #[must_use]
    pub fn is_available(self) -> bool {
        self != Self::Unavailable
    }

    /// Whether a value of this quality may be the basis of *active* steering
    /// or other interventions. Only confirmed values can be. Estimated
    /// values are for display, map matching hints and warnings that a human
    /// can disregard.
    #[must_use]
    pub fn is_suitable_for_active_steering(self) -> bool {
        self == Self::Confirmed
    }

    /// The lower of two qualities: a result derived from both is no better
    /// than the weaker input.
    #[must_use]
    pub fn weakest(self, other: Self) -> Self {
        self.max(other)
    }
}

#[cfg(test)]
mod tests {
    use super::Quality;

    #[test]
    fn names_round_trip() {
        for q in [Quality::Confirmed, Quality::Estimated, Quality::Unavailable] {
            assert_eq!(Quality::parse(q.as_str()), Some(q));
        }
        assert_eq!(Quality::parse("Confirmed"), None);
        assert_eq!(Quality::parse(""), None);
    }

    #[test]
    fn only_confirmed_values_may_steer() {
        assert!(Quality::Confirmed.is_suitable_for_active_steering());
        assert!(!Quality::Estimated.is_suitable_for_active_steering());
        assert!(!Quality::Unavailable.is_suitable_for_active_steering());
        assert!(!Quality::Unavailable.is_available());
    }

    #[test]
    fn weakest_never_improves() {
        assert_eq!(
            Quality::Confirmed.weakest(Quality::Estimated),
            Quality::Estimated
        );
        assert_eq!(
            Quality::Unavailable.weakest(Quality::Confirmed),
            Quality::Unavailable
        );
        assert_eq!(
            Quality::Confirmed.weakest(Quality::Confirmed),
            Quality::Confirmed
        );
    }
}
