#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitProfileKey {
    Standing,
    WorkingTall,
    WorkingWide,
    Sleeping,
    Error,
    Mini,
}

#[derive(Debug, Clone, Copy)]
pub struct RelativeHitRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct HitProfile {
    pub rects: &'static [RelativeHitRect],
}

const STANDING_RECTS: &[RelativeHitRect] = &[
    RelativeHitRect {
        x: 0.27,
        y: 0.20,
        width: 0.46,
        height: 0.50,
    },
    RelativeHitRect {
        x: 0.20,
        y: 0.56,
        width: 0.60,
        height: 0.30,
    },
];

const WORKING_WIDE_RECTS: &[RelativeHitRect] = &[
    RelativeHitRect {
        x: 0.14,
        y: 0.42,
        width: 0.72,
        height: 0.28,
    },
    RelativeHitRect {
        x: 0.22,
        y: 0.24,
        width: 0.56,
        height: 0.26,
    },
];

const SLEEPING_RECTS: &[RelativeHitRect] = &[
    RelativeHitRect {
        x: 0.16,
        y: 0.56,
        width: 0.68,
        height: 0.22,
    },
    RelativeHitRect {
        x: 0.30,
        y: 0.44,
        width: 0.30,
        height: 0.18,
    },
];

// Mini mode: cover the visible portion generously.
// In mini mode ~48.6% of the pet is visible (MINI_OFFSET_RATIO),
// so the hit region must span the exposed side of the window.
const MINI_RECTS: &[RelativeHitRect] = &[RelativeHitRect {
    x: 0.05,
    y: 0.10,
    width: 0.90,
    height: 0.80,
}];

pub fn profile_for_svg(svg: &str) -> HitProfileKey {
    match svg {
        "clyde-mini-idle.svg"
        | "clyde-mini-alert.svg"
        | "clyde-mini-crabwalk.svg"
        | "clyde-mini-enter-sleep.svg"
        | "clyde-mini-enter.svg"
        | "clyde-mini-happy.svg"
        | "clyde-mini-peek.svg"
        | "clyde-mini-sleep.svg" => HitProfileKey::Mini,

        "clyde-sleeping.svg"
        | "clyde-idle-doze.svg"
        | "clyde-idle-collapse.svg"
        | "clyde-collapse-sleep.svg"
        | "clyde-wake.svg" => HitProfileKey::Sleeping,

        "clyde-working-juggling.svg"
        | "clyde-working-carrying.svg"
        | "clyde-working-sweeping.svg"
        | "clyde-working-conducting.svg" => HitProfileKey::WorkingWide,

        "clyde-working-typing.svg"
        | "clyde-working-thinking.svg"
        | "clyde-working-ultrathink.svg"
        | "clyde-working-wizard.svg"
        | "clyde-working-building.svg"
        | "clyde-working-debugger.svg" => HitProfileKey::WorkingTall,

        "clyde-error.svg" => HitProfileKey::Error,

        _ => HitProfileKey::Standing,
    }
}

pub fn profile(key: HitProfileKey) -> HitProfile {
    let rects = match key {
        HitProfileKey::Standing | HitProfileKey::WorkingTall | HitProfileKey::Error => {
            STANDING_RECTS
        }
        HitProfileKey::WorkingWide => WORKING_WIDE_RECTS,
        HitProfileKey::Sleeping => SLEEPING_RECTS,
        HitProfileKey::Mini => MINI_RECTS,
    };

    HitProfile { rects }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_for_svg_maps_mini_pose() {
        assert_eq!(profile_for_svg("clyde-mini-idle.svg"), HitProfileKey::Mini);
    }

    #[test]
    fn test_profile_for_svg_maps_sleep_pose() {
        assert_eq!(profile_for_svg("clyde-sleeping.svg"), HitProfileKey::Sleeping);
    }

    #[test]
    fn test_profile_for_svg_maps_wide_work_pose() {
        assert_eq!(profile_for_svg("clyde-working-juggling.svg"), HitProfileKey::WorkingWide);
    }
}
