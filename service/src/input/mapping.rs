use core::fmt;

pub const PAD_LAYOUT_ID: &str = "console16-12btn-v1";
pub const PAD_LAYOUT_VERSION: u16 = 1;
pub const PAD_BUTTON_COUNT: usize = 12;
pub const PAD_MASK: u16 = 0x0fff;
pub const RESERVED_MASK: u16 = !PAD_MASK;
pub const PLAYER_ONE_PORT: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PadButton {
    A,
    B,
    X,
    Y,
    L,
    R,
    Up,
    Down,
    Left,
    Right,
    Start,
    Select,
}

impl PadButton {
    pub const ALL: [Self; PAD_BUTTON_COUNT] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::L,
        Self::R,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Start,
        Self::Select,
    ];

    pub const fn bit(self) -> u8 {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::X => 2,
            Self::Y => 3,
            Self::L => 4,
            Self::R => 5,
            Self::Up => 6,
            Self::Down => 7,
            Self::Left => 8,
            Self::Right => 9,
            Self::Start => 10,
            Self::Select => 11,
        }
    }

    pub const fn mask(self) -> u16 {
        1u16 << self.bit()
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::L => "L",
            Self::R => "R",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Start => "Start",
            Self::Select => "Select",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "A" => Some(Self::A),
            "B" => Some(Self::B),
            "X" => Some(Self::X),
            "Y" => Some(Self::Y),
            "L" => Some(Self::L),
            "R" => Some(Self::R),
            "Up" => Some(Self::Up),
            "Down" => Some(Self::Down),
            "Left" => Some(Self::Left),
            "Right" => Some(Self::Right),
            "Start" => Some(Self::Start),
            "Select" => Some(Self::Select),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PadWord(u16);

impl PadWord {
    pub const ZERO: Self = Self(0);

    pub const fn raw(self) -> u16 {
        self.0
    }

    pub const fn into_u32_buttons(self) -> u32 {
        self.0 as u32
    }

    pub fn new(raw: u16) -> Result<Self, PadWordError> {
        let reserved = raw & RESERVED_MASK;
        if reserved != 0 {
            return Err(PadWordError::ReservedBitsSet { raw, reserved });
        }

        Ok(Self(raw))
    }

    pub fn from_buttons(buttons: impl IntoIterator<Item = PadButton>) -> Self {
        let raw = buttons
            .into_iter()
            .fold(0, |word, button| word | button.mask());
        Self(neutralize_opposites(raw))
    }

    pub fn merge(keyboard: Self, gamepad: Self) -> Self {
        Self(neutralize_opposites(keyboard.raw() | gamepad.raw()))
    }

    pub fn merge_buttons(
        keyboard: impl IntoIterator<Item = PadButton>,
        gamepad: impl IntoIterator<Item = PadButton>,
    ) -> Self {
        Self::merge(Self::from_buttons(keyboard), Self::from_buttons(gamepad))
    }

    pub const fn contains(self, button: PadButton) -> bool {
        self.0 & button.mask() != 0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn buttons(self) -> Vec<PadButton> {
        PadButton::ALL
            .iter()
            .copied()
            .filter(|button| self.contains(*button))
            .collect()
    }

    pub fn button_names(self) -> Vec<&'static str> {
        self.buttons().into_iter().map(PadButton::name).collect()
    }
}

impl TryFrom<u16> for PadWord {
    type Error = PadWordError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PadWord> for u16 {
    fn from(value: PadWord) -> Self {
        value.raw()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadWordError {
    ReservedBitsSet { raw: u16, reserved: u16 },
}

impl fmt::Display for PadWordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedBitsSet { reserved, .. } => {
                write!(f, "pad word has reserved bits set: {reserved:#06x}")
            }
        }
    }
}

impl std::error::Error for PadWordError {}

const fn neutralize_opposites(raw: u16) -> u16 {
    let mut neutralized = raw & PAD_MASK;
    let vertical = PadButton::Up.mask() | PadButton::Down.mask();
    let horizontal = PadButton::Left.mask() | PadButton::Right.mask();

    if neutralized & vertical == vertical {
        neutralized &= !vertical;
    }

    if neutralized & horizontal == horizontal {
        neutralized &= !horizontal;
    }

    neutralized
}
