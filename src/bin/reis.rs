use ashpd::desktop::input_capture::{Barrier, Capabilities, InputCapture};
use futures::StreamExt;
use reis::{
    ei::{self, keyboard::KeyState},
    event::{DeviceCapability, EiEvent, KeyboardKey},
    tokio::{EiConvertEventStream, EiEventStream},
};
use std::{collections::HashMap, os::unix::net::UnixStream, sync::OnceLock, time::Duration};

#[allow(unused)]
enum Position {
    Left,
    Right,
    Top,
    Bottom,
}

static INTERFACES: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();

#[tokio::main(flavor = "current_thread")]
async fn main() -> ashpd::Result<()> {
    // terminate automatically after 10 seconds
    tokio::task::spawn(async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        std::process::exit(1);
    });

    let input_capture = InputCapture::new().await?;

    let (session, _cap) = input_capture
        .create_session(
            &ashpd::WindowIdentifier::default(),
            Capabilities::Keyboard | Capabilities::Pointer | Capabilities::Touchscreen,
        )
        .await?;

    // connect to eis server
    eprintln!("connect_to_eis");
    let fd = input_capture.connect_to_eis(&session).await?;

    // create unix stream from fd
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(true)?;

    // create ei context
    let context = ei::Context::new(stream)?;
    context.flush().unwrap();

    let mut event_stream = EiEventStream::new(context.clone())?;
    let interfaces = INTERFACES.get_or_init(|| {
        HashMap::from([
            ("ei_connection", 1),
            ("ei_callback", 1),
            ("ei_pingpong", 1),
            ("ei_seat", 1),
            ("ei_device", 2),
            ("ei_pointer", 1),
            ("ei_pointer_absolute", 1),
            ("ei_scroll", 1),
            ("ei_button", 1),
            ("ei_keyboard", 1),
            ("ei_touchscreen", 1),
        ])
    });
    let response = reis::tokio::ei_handshake(
        &mut event_stream,
        "ashpd-mre",
        ei::handshake::ContextType::Receiver,
        interfaces,
    )
    .await
    .expect("ei handshake failed");

    let mut event_stream = EiConvertEventStream::new(event_stream, response.serial);

    let pos = Position::Left;
    let zones = input_capture.zones(&session).await?.response()?;
    eprintln!("zones: {zones:?}");
    let barriers = zones
        .regions()
        .iter()
        .enumerate()
        .map(|(n, r)| {
            let id = (n + 1) as u32;
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

    input_capture.enable(&session).await?;

    let mut activate_stream = input_capture.receive_activated().await?;

    loop {
        let activated = activate_stream.next().await.unwrap();

        eprintln!("activated: {activated:?}");
        loop {
            let ei_event = event_stream.next().await.unwrap().unwrap();
            eprintln!("ei event: {ei_event:?}");
            if let EiEvent::SeatAdded(seat_event) = &ei_event {
                seat_event.seat.bind_capabilities(&[
                    DeviceCapability::Pointer,
                    DeviceCapability::PointerAbsolute,
                    DeviceCapability::Keyboard,
                    DeviceCapability::Touch,
                    DeviceCapability::Scroll,
                    DeviceCapability::Button,
                ]);
                context.flush().unwrap();
            }
            if let EiEvent::DeviceAdded(_) = ei_event {
                // new device added -> restart capture
                break;
            };
            if let EiEvent::KeyboardKey(KeyboardKey { key, state, .. }) = ei_event {
                if key == 1 && state == KeyState::Press {
                    // esc pressed
                    break;
                }
            }
        }

        eprintln!("releasing input capture");
        let (x, y) = activated.cursor_position().unwrap();
        let (x, y) = (x as f64, y as f64);
        let cursor_pos = match pos {
            Position::Left => (x + 1., y),
            Position::Right => (x - 1., y),
            Position::Top => (x, y - 1.),
            Position::Bottom => (x, y + 1.),
        };
        input_capture
            .release(&session, activated.activation_id(), Some(cursor_pos))
            .await?;
    }
}
