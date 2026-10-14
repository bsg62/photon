//! The system's video backends (`photon_ui::video`), over three small files: `h264.mp4` and
//! `hevc.mp4`, 320 by 240, four seconds of red, green, blue and white, a second each, with
//! sound - so a frame's colour says which second it came from; and `rotated.mp4`, stored
//! 320 by 240 with its left half red and its right half blue, and a matrix that turns it a
//! quarter anticlockwise as it is shown: blue over red, 240 by 320.
//!
//! A binary of its own, with no test harness of libtest's: every test runs on the main
//! thread, which AVFoundation's player wants (`libtest-mimic`, one thread). A system that
//! cannot decode what a test needs - a Linux without GStreamer's plugins, a Windows without
//! Microsoft's HEVC extension - is said so in the output, and the test passes over itself.
//! A file the system calls broken is a failure.

use libtest_mimic::{Arguments, Failed, Trial};
use photon_ui::video::{self, Failure, Picture, Player};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() {
    let mut args = Arguments::from_args();
    args.test_threads = Some(1);
    let tests = vec![
        Trial::test("h264_posters_show_the_second_asked_for", || {
            posters("h264.mp4")
        }),
        Trial::test(
            "hevc_posters_show_the_second_asked_for_where_hevc_is_decoded",
            || posters("hevc.mp4"),
        ),
        Trial::test("a_turned_video_is_turned_as_it_is_seen", turned),
        Trial::test("a_video_plays_its_seconds_in_order", in_order),
        Trial::test("a_paused_video_stands_still", paused),
        Trial::test("a_seek_lands_on_the_second_asked_for", seek),
        Trial::test("a_video_knows_its_length_and_its_end", length_and_end),
        Trial::test("volume_and_mute_are_set_without_a_failure", sound),
        Trial::test("a_file_that_is_no_video_is_a_failure_not_a_panic", no_video),
    ];
    libtest_mimic::run(&args, tests).exit();
}

fn media(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/video")
        .join(name)
}

/// What the system cannot do, said and passed over.
fn cannot(why: impl std::fmt::Display) -> Result<(), Failed> {
    println!("passed over: {why}");
    Ok(())
}

/// The colour of a pixel, named by which channels are lit.
fn named(px: [u8; 4]) -> &'static str {
    match [px[0], px[1], px[2]].map(|c| c > 128) {
        [true, false, false] => "red",
        [false, true, false] => "green",
        [false, false, true] => "blue",
        [true, true, true] => "white",
        _ => "other",
    }
}

fn centre(picture: &Picture) -> &'static str {
    named(picture.pixel(picture.width / 2, picture.height / 2))
}

/// What the coloured files show at `at` seconds.
fn expected(at: f64) -> &'static str {
    ["red", "green", "blue", "white"][(at.max(0.0) as usize).min(3)]
}

/// A frame shown at `at` with `colour` is the second it is from - or, within a tenth of a
/// second after a second's edge, the second before: the frame's time is the player's own,
/// and GStreamer's runs two frames behind the picture in a file with an edit list.
fn in_its_second(at: f64, colour: &str) -> bool {
    colour == expected(at) || (at.fract() < 0.1 && at >= 1.0 && colour == expected(at - 1.0))
}

fn check(ok: bool, said: impl FnOnce() -> String) -> Result<(), Failed> {
    if ok { Ok(()) } else { Err(said().into()) }
}

fn posters(file: &str) -> Result<(), Failed> {
    if !video::can_play() {
        return cannot("the system has no H.264 decoder");
    }
    for at in [0.4, 2.5] {
        match video::poster(&media(file), at) {
            Ok(poster) => check(
                (poster.width, poster.height) == (320, 240) && centre(&poster) == expected(at),
                || format!("{file} at {at} s: {poster:?}, {}", centre(&poster)),
            )?,
            Err(Failure::Unsupported(why)) if file != "h264.mp4" => return cannot(why),
            Err(failure) => return Err(format!("{file} at {at} s: {failure:?}").into()),
        }
    }
    Ok(())
}

fn opened(file: &str) -> Result<Option<Player>, Failed> {
    if !video::can_play() {
        cannot("the system has no H.264 decoder")?;
        return Ok(None);
    }
    Player::open(&media(file))
        .map(Some)
        .map_err(|failure| format!("{file}: {failure:?}").into())
}

/// The frames a player hands over for `long`, each asked for every five milliseconds.
fn frames_for(player: &Player, long: Duration) -> Vec<Picture> {
    let until = Instant::now() + long;
    let mut frames = Vec::new();
    while Instant::now() < until {
        frames.extend(player.frame());
        std::thread::sleep(Duration::from_millis(5));
    }
    frames
}

/// The first frame handed over at or after `at` seconds, within `long`.
fn frame_from(player: &Player, at: f64, long: Duration) -> Option<Picture> {
    let until = Instant::now() + long;
    while Instant::now() < until {
        if let Some(frame) = player.frame().filter(|frame| frame.at >= at) {
            return Some(frame);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

fn ends(picture: &Picture) -> (&'static str, &'static str) {
    let x = picture.width / 2;
    (
        named(picture.pixel(x, picture.height / 4)),
        named(picture.pixel(x, picture.height * 3 / 4)),
    )
}

fn turned() -> Result<(), Failed> {
    if !video::can_play() {
        return cannot("the system has no H.264 decoder");
    }
    let poster = video::poster(&media("rotated.mp4"), 1.0)
        .map_err(|failure| format!("poster: {failure:?}"))?;
    check(
        (poster.width, poster.height) == (240, 320) && ends(&poster) == ("blue", "red"),
        || format!("poster {poster:?}, top and bottom {:?}", ends(&poster)),
    )?;
    let Some(player) = opened("rotated.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    let frames = frames_for(&player, Duration::from_millis(500));
    let last = frames.last().ok_or("no frame played")?;
    check(
        (last.width, last.height) == (240, 320) && ends(last) == ("blue", "red"),
        || format!("played {last:?}, top and bottom {:?}", ends(last)),
    )
}

fn in_order() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    let frames = frames_for(&player, Duration::from_millis(2500));
    let seen: Vec<_> = frames
        .iter()
        .map(|frame| (frame.at, centre(frame)))
        .collect();
    let wrong: Vec<_> = seen
        .iter()
        .filter(|(at, colour)| !in_its_second(*at, colour))
        .collect();
    let first = seen.first().copied();
    check(
        frames.len() >= 20
            && wrong.is_empty()
            && first.is_some_and(|(at, colour)| at >= 0.0 && colour == "red")
            && seen.iter().any(|(_, colour)| *colour == "green")
            && seen.windows(2).all(|pair| pair[0].0 < pair[1].0)
            && frames
                .iter()
                .all(|frame| (frame.width, frame.height) == (320, 240)),
        || format!("{} frames, first {first:?}, wrong {wrong:?}", frames.len()),
    )
}

fn paused() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    frames_for(&player, Duration::from_millis(600));
    let playing = player.position().ok_or("no position while playing")?;
    player.pause();
    frames_for(&player, Duration::from_millis(100));
    let stopped = player.position().ok_or("no position once paused")?;
    frames_for(&player, Duration::from_millis(400));
    let later = player.position().ok_or("no position later")?;
    check(
        playing > 0.2 && stopped >= playing && (later - stopped).abs() < 0.05,
        || format!("playing at {playing}, paused at {stopped}, then {later}"),
    )
}

fn seek() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_muted(true);
    player.play();
    player.seek(3.5);
    let frame = frame_from(&player, 3.4, Duration::from_secs(3)).ok_or("no frame after 3.4 s")?;
    check(centre(&frame) == "white", || {
        format!("{frame:?} is {}", centre(&frame))
    })
}

fn length_and_end() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    let whole = player.duration().ok_or("no duration")?;
    check((whole - 4.0).abs() < 0.1, || format!("{whole} s long"))?;
    check(!player.ended(), || "ended before it played".into())?;
    player.set_muted(true);
    player.play();
    player.seek(3.7);
    let until = Instant::now() + Duration::from_secs(3);
    while !player.ended() {
        if Instant::now() > until {
            return Err(format!("not ended, at {:?}", player.position()).into());
        }
        player.frame();
        std::thread::sleep(Duration::from_millis(10));
    }
    check(player.failure().is_none(), || {
        format!("ended with {:?}", player.failure())
    })
}

fn sound() -> Result<(), Failed> {
    let Some(player) = opened("h264.mp4")? else {
        return Ok(());
    };
    player.set_volume(0.3);
    player.set_muted(true);
    player.play();
    let frames = frames_for(&player, Duration::from_millis(400));
    player.set_muted(false);
    player.set_volume(0.0);
    frames_for(&player, Duration::from_millis(100));
    check(!frames.is_empty() && player.failure().is_none(), || {
        format!("{} frames, {:?}", frames.len(), player.failure())
    })
}

fn no_video() -> Result<(), Failed> {
    let dir = tempfile::tempdir()?;
    let fake = dir.path().join("fake.mp4");
    std::fs::write(&fake, b"this is no video, whatever its name says")?;
    let missing = dir.path().join("missing.mp4");
    for path in [&fake, &missing] {
        check(video::poster(path, 0.0).is_err(), || {
            format!("a poster of {}", path.display())
        })?;
        check(Player::open(path).is_err(), || {
            format!("a player of {}", path.display())
        })?;
    }
    Ok(())
}
