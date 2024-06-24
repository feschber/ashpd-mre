
use anyhow::{anyhow, Result};
use ashpd::desktop::input_capture::{Barrier, Capabilities, InputCapture, Zones};
use futures::StreamExt;
use reis::{
    ei,
    event::{DeviceCapability, EiEvent},
    tokio::{EiConvertEventStream, EiEventStream},
};
use std::{collections::HashMap, os::unix::net::UnixStream, sync::OnceLock, time::Duration};

pub enum Position {
    Left,
    Right,
    Top,
    Bottom,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    ic().await.unwrap();
}

static INTERFACES: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();

fn select_barriers(zones: &Zones, pos: Position) -> Vec<Barrier> {
    zones
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
                Position::Bottom => (x, y + height - 1, x + width - 1, y + height - 1),
            };
            Barrier::new(id, barrier_pos)
        })
        .collect()
}

async fn ic() -> Result<()> {
    // terminate automatically after 10 seconds
    tokio::task::spawn(async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        std::process::exit(1);
    });

    let input_capture = InputCapture::new().await.unwrap();

    // create input capture session
    log::info!("creating input capture session");
    let (session, _cap) = input_capture
        .create_session(
            &ashpd::WindowIdentifier::default(),
            Capabilities::Keyboard | Capabilities::Pointer | Capabilities::Touchscreen,
        )
        .await?;

    // connect to eis server
    log::info!("connect_to_eis");
    let fd = input_capture.connect_to_eis(&session).await?;

    // create unix stream from fd
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(true)?;

    // create ei context
    let context = ei::Context::new(stream)?;
    context.flush()?;

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
    let response = match reis::tokio::ei_handshake(
        &mut event_stream,
        "ashpd-mre",
        ei::handshake::ContextType::Receiver,
        interfaces,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => return Err(anyhow!("ei handshake failed: {e:?}")),
    };

    let mut event_stream = EiConvertEventStream::new(event_stream, response.serial);

    log::info!("selecting zones");
    let zones = input_capture.zones(&session).await?.response()?;
    log::info!("{zones:?}");
    // FIXME: position
    let barriers = select_barriers(&zones, Position::Left);

    log::info!("selecting barriers: {barriers:?}");
    input_capture
        .set_pointer_barriers(&session, &barriers, zones.zone_set())
        .await?;

    log::info!("enabling session");
    input_capture.enable(&session).await?;

    let mut activate_stream = input_capture.receive_activated().await?;

    for i in 0.. {
        // disable and reenable after 6th round
        if i == 6 {
            // I dont know why this causes input capture to no longer send any events...
            log::info!("-------------DISABLE-------------");
            input_capture.disable(&session).await?;
            log::info!("-------------ENABLE--------------");
            input_capture.enable(&session).await?;
        }

        log::error!("CAPTURE {i}");
        let activated = activate_stream
            .next()
            .await
            .ok_or(anyhow!("expected activate signal"))?;
        log::info!("activated: {activated:?}");
        let mut i = 0;
        loop {
            let ei_event = event_stream.next().await.unwrap().unwrap();
            log::info!("ei event: {ei_event:?}");
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
            if let EiEvent::DeviceAdded(_) = &ei_event {
                break;
            }

            /* just for debugging break out of the loop after 100 events */
            i += 1;
            if i == 20 {
                break;
            }
        }

        log::info!("releasing input capture");
        let (x, y) = activated.cursor_position().unwrap();
        let cp = (x as f64 + 10., y as f64);
        input_capture
            .release(&session, activated.activation_id(), Some(cp))
            .await
            .unwrap();
    }

    Ok(())
}
