use rom_operator_bridge_service::framebuffer::{
    RawFramebuffer, RawFramebufferFormat, SYNTHETIC_FRAME_HEIGHT, SYNTHETIC_FRAME_WIDTH,
    framebuffer_png, rgb8_png, synthetic_frame_png, xrgb8888_to_rgb8,
};

#[test]
fn synthetic_png_matches_rgb_encoder_fixture() {
    let mut rgb = Vec::with_capacity((SYNTHETIC_FRAME_WIDTH * SYNTHETIC_FRAME_HEIGHT * 3) as usize);
    for y in 0..SYNTHETIC_FRAME_HEIGHT {
        for x in 0..SYNTHETIC_FRAME_WIDTH {
            rgb.push(x as u8);
            rgb.push((y as u8).wrapping_mul(2));
            rgb.push((x ^ y) as u8);
        }
    }

    assert_eq!(
        synthetic_frame_png(0),
        rgb8_png(SYNTHETIC_FRAME_WIDTH, SYNTHETIC_FRAME_HEIGHT, &rgb)
            .expect("fixture dimensions are valid")
    );
}

#[test]
fn xrgb8888_conversion_strips_x_byte_and_preserves_rgb_order() {
    let pixels = [
        0x30, 0x20, 0x10, 0xaa, // B, G, R, X
        0x60, 0x50, 0x40, 0xbb,
    ];

    let rgb = xrgb8888_to_rgb8(RawFramebuffer {
        width: 2,
        height: 1,
        stride: 8,
        format: RawFramebufferFormat::Xrgb8888,
        pixels: &pixels,
    })
    .expect("xrgb fixture converts");

    assert_eq!(rgb, [0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
}

#[test]
fn xrgb8888_conversion_ignores_row_padding() {
    let pixels = [
        0x03, 0x02, 0x01, 0x00, 0x06, 0x05, 0x04, 0x00, 0xf0, 0xf1, 0xf2, 0xf3, 0x09, 0x08, 0x07,
        0x00, 0x0c, 0x0b, 0x0a, 0x00, 0xf4, 0xf5, 0xf6, 0xf7,
    ];

    let rgb = xrgb8888_to_rgb8(RawFramebuffer {
        width: 2,
        height: 2,
        stride: 12,
        format: RawFramebufferFormat::Xrgb8888,
        pixels: &pixels,
    })
    .expect("padded fixture converts");

    assert_eq!(
        rgb,
        [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c
        ]
    );
}

#[test]
fn conversion_rejects_invalid_dimensions_stride_and_lengths() {
    let pixels = [0u8; 16];
    for raw in [
        RawFramebuffer {
            width: 0,
            height: 1,
            stride: 4,
            format: RawFramebufferFormat::Xrgb8888,
            pixels: &pixels[..4],
        },
        RawFramebuffer {
            width: 2,
            height: 1,
            stride: 4,
            format: RawFramebufferFormat::Xrgb8888,
            pixels: &pixels[..4],
        },
        RawFramebuffer {
            width: 2,
            height: 2,
            stride: 8,
            format: RawFramebufferFormat::Xrgb8888,
            pixels: &pixels[..8],
        },
        RawFramebuffer {
            width: u32::MAX,
            height: u32::MAX,
            stride: u32::MAX,
            format: RawFramebufferFormat::Xrgb8888,
            pixels: &[],
        },
    ] {
        assert!(xrgb8888_to_rgb8(raw).is_err());
    }
}

#[test]
fn route_facing_framebuffer_png_requires_runtime_schema_dimensions() {
    let small_pixels = [0u8; 8];
    assert!(
        framebuffer_png(RawFramebuffer {
            width: 2,
            height: 1,
            stride: 8,
            format: RawFramebufferFormat::Xrgb8888,
            pixels: &small_pixels,
        })
        .is_err()
    );

    let valid_pixels = vec![0u8; (SYNTHETIC_FRAME_WIDTH * SYNTHETIC_FRAME_HEIGHT * 4) as usize];
    let png = framebuffer_png(RawFramebuffer {
        width: SYNTHETIC_FRAME_WIDTH,
        height: SYNTHETIC_FRAME_HEIGHT,
        stride: SYNTHETIC_FRAME_WIDTH * 4,
        format: RawFramebufferFormat::Xrgb8888,
        pixels: &valid_pixels,
    })
    .expect("runtime schema dimensions convert");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}
