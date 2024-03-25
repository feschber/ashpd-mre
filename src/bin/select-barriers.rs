use ashpd::desktop::input_capture::{Barrier, Capabilities, InputCapture};

#[allow(unused)]
enum Position {
    Left,
    Right,
    Top,
    Bottom,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ashpd::Result<()> {
    let input_capture = InputCapture::new().await?;
    let (session, _capabilities) = input_capture
        .create_session(
            &ashpd::WindowIdentifier::default(),
            Capabilities::Keyboard | Capabilities::Pointer | Capabilities::Touchscreen,
        )
        .await?;

    let pos = Position::Bottom;
    let zones = input_capture.zones(&session).await?.response()?;
    eprintln!("zones: {zones:?}");
    let barriers = zones
        .regions()
        .iter()
        .enumerate()
        .map(|(n, r)| {
            let id = n as u32;
            let (x, y) = (r.x_offset(), r.y_offset());
            let (width, height) = (r.width() as i32, r.height() as i32);
            let barrier_pos = match pos {
                Position::Left => (x, y, x, y + height - 1), // start pos, end pos, inclusive
                Position::Right => (x + width, y, x + width, y + height - 1),
                Position::Top => (x, y, x + width - 1, y),
                Position::Bottom => (x, y + height, x + width - 1, y + height),
            };
            Barrier::new(id, barrier_pos)
        })
        .collect::<Vec<_>>();

    eprintln!("requested barriers: {barriers:?}");

    let request = input_capture
        .set_pointer_barriers(&session, &barriers, zones.zone_set())
        .await?;
    let response = request.response()?;
    let failed_barrier_ids = response.failed_barriers();

    eprintln!("failed barrier ids: {:?}", failed_barrier_ids);

    // barriers can fail for various reasons, mostly wrong placement.
    // For further information, the logs of the active portal can be useful:
    // ```sh
    // journalctl --user -xeu xdg-desktop-portal-gnome.service
    // ```

    Ok(())
}
