use rom_operator_bridge_service::input::{
    PAD_LAYOUT_ID, PAD_LAYOUT_VERSION, PAD_MASK, PLAYER_ONE_PORT, PadButton, PadWord, PadWordError,
    RESERVED_MASK,
};

#[test]
fn exposes_frozen_layout_constants() {
    assert_eq!(PAD_LAYOUT_ID, "console16-12btn-v1");
    assert_eq!(PAD_LAYOUT_VERSION, 1);
    assert_eq!(PAD_MASK, 0x0fff);
    assert_eq!(RESERVED_MASK, 0xf000);
    assert_eq!(PLAYER_ONE_PORT, 0);
}

#[test]
fn maps_every_button_to_its_frozen_bit() {
    let expected = [
        (PadButton::A, 0),
        (PadButton::B, 1),
        (PadButton::X, 2),
        (PadButton::Y, 3),
        (PadButton::L, 4),
        (PadButton::R, 5),
        (PadButton::Up, 6),
        (PadButton::Down, 7),
        (PadButton::Left, 8),
        (PadButton::Right, 9),
        (PadButton::Start, 10),
        (PadButton::Select, 11),
    ];

    for (button, bit) in expected {
        assert_eq!(button.bit(), bit);
        assert_eq!(button.mask(), 1u16 << bit);
        assert_eq!(PadWord::from_buttons([button]).raw(), 1u16 << bit);
        assert_eq!(PadButton::from_name(button.name()), Some(button));
    }
}

#[test]
fn rejects_reserved_bits_in_raw_words() {
    for bit in 12..=15 {
        let raw = 1u16 << bit;
        assert_eq!(
            PadWord::new(raw),
            Err(PadWordError::ReservedBitsSet { raw, reserved: raw })
        );
    }

    assert_eq!(
        PadWord::new(0xf123),
        Err(PadWordError::ReservedBitsSet {
            raw: 0xf123,
            reserved: 0xf000
        })
    );
}

#[test]
fn preserves_valid_raw_low_bits() {
    let raw = PadButton::Up.mask() | PadButton::Down.mask() | PadButton::A.mask();

    assert_eq!(
        PadWord::new(raw).expect("reserved bits are clear").raw(),
        raw
    );
}

#[test]
fn constructed_button_words_never_set_reserved_bits() {
    let all_buttons = PadWord::from_buttons(PadButton::ALL);
    assert_eq!(all_buttons.raw() & RESERVED_MASK, 0);
    assert!(all_buttons.raw() <= PAD_MASK);
}

#[test]
fn neutralizes_opposite_dpad_directions() {
    assert!(PadWord::from_buttons([PadButton::Up, PadButton::Down]).is_zero());
    assert!(PadWord::from_buttons([PadButton::Left, PadButton::Right]).is_zero());

    let mixed = PadWord::from_buttons([
        PadButton::A,
        PadButton::Up,
        PadButton::Down,
        PadButton::Left,
    ]);

    assert_eq!(mixed.raw(), PadButton::A.mask() | PadButton::Left.mask());
}

#[test]
fn merges_keyboard_and_gamepad_with_union_then_neutralization() {
    let keyboard = [PadButton::A, PadButton::Up, PadButton::Start];
    let gamepad = [PadButton::B, PadButton::Down, PadButton::Right];

    let merged = PadWord::merge_buttons(keyboard, gamepad);

    assert!(merged.contains(PadButton::A));
    assert!(merged.contains(PadButton::B));
    assert!(merged.contains(PadButton::Start));
    assert!(merged.contains(PadButton::Right));
    assert!(!merged.contains(PadButton::Up));
    assert!(!merged.contains(PadButton::Down));
}

#[test]
fn returns_sorted_button_names_by_layout_bit_order() {
    let word = PadWord::new(
        PadButton::Select.mask()
            | PadButton::A.mask()
            | PadButton::Right.mask()
            | PadButton::L.mask(),
    )
    .expect("reserved bits are clear");

    assert_eq!(word.button_names(), ["A", "L", "Right", "Select"]);
}

#[test]
fn converts_valid_pad_word_to_hypervisor_buttons_width() {
    let word = PadWord::from_buttons([PadButton::A, PadButton::Start]);

    assert_eq!(word.raw(), 0x0401);
    assert_eq!(word.into_u32_buttons(), 0x0401);
}
