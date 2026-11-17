# Shared game capture

The aLogs app captures the game once for every module (the meter, Sanctum Helper, later ones) and shares the
pictures through the named shared memory `Local\aLogs.Capture.v2`. Writer: `crates/alogs-app/src/capture.rs` (with
`gpu_capture.rs`). Readers: the meter's `crates/alogs-capture/src/shared.rs` (`SharedCapture`) and Sanctum Helper's
`crates/alogs-sanctum/src/shared_capture.rs` (ports of B16's `bossbar/wincapture.py` and Sanctum Helper's
`SharedCapture.cs`).

## How pictures are taken
Both ways keep the picture on the graphics card and copy into memory only the areas readers ask for:

1. **Window capture** (Windows Graphics Capture), only where the yellow border can be turned off (Windows 11).
   Just the game window, so our overlays are never in it.
2. **Desktop Duplication** of the screen the game is on, only while the game is in front. Our overlays can be in
   it (flags bit 1): the meter and Sanctum Helper hide their windows from capture then.

Not in front on Windows 10: while no window shows over the game, Desktop Duplication goes on. With a window over
the game, window capture takes the game window alone, with Windows 10's yellow border (behind whatever covers the
game), until the game is uncovered or in front again. PrintWindow is only the fallback where that can't start or
shows nothing: ~50 ms per 4K frame, and each copy makes the game wait (measured 15 Nov: about 220 doubled frames a
minute at 4 copies a second). BitBlt screen copies (v1) were removed.

## Reading
1. Open the mapping (map it whole: size 0) and check the magic `ALCP`.
2. Claim a request slot (64 + 16·i): your pid, or one whose "wanted until" has passed. Keep "wanted until" ~2.5 s
   ahead and the interval (ms) at the pace you read.
3. In your picture slot (320 + 64·i) write the area you need as shares of the client size (0–65535; the box around
   everything you read in the last couple of seconds) and a new request id whenever the area changes.
4. When `served id == request id`, the slot's seq is even and the picture time is fresh, copy your part from the
   pool (4096 + offset, stride, at client position x, y), then check the seq again.

A reader that needs several separate parts of the frame (since B8) writes up to 7 of them (shares, as above) at
2056 + 120·i, their count first, then the box around them all in the picture slot and the new request id. The app
copies each part in the same pass and writes them one after another from the slot's offset (stride 0; each part
w·4 bytes a row), with each part's x, y, w, h in client pixels next to its shares. An app from before B8 ignores the
parts and sends the box around them, so the reader handles both (alogs-capture's `read_rects`). The meter uses
this for screen reads between fights: the top band plus the screens' text boxes, about half of a 4K frame.

## Overlays in captures (B9)
aLogs' overlays stay in screenshots and recordings. Where the picture is a copy of the screen (Desktop Duplication),
each program lists where its overlay windows are (1352 + 144·owner: updated time, pid, count, up to 8 screen rects;
owners 0 the meter, 1 Sanctum Helper, 2 the app's button) and every reader covers all of them in its own picture
before reading (the meter in magenta, which its bar readers look past; Sanctum Helper in black). A block not updated
for 3 s is stale. While the meter takes a leaderboard proof it sets "clean until" (1344, unix ms) and every program
keeps its overlays out of screen captures until then, so a proof is the game alone. A Sanctum Helper map drawn over
the game itself isn't listed: it covers what Sanctum Helper reads, so it stays out of screen copies.

While a result, Party or menu screen is up on Windows 10, the meter asks for the game window alone by keeping "game only
until" (1792, unix ms) a few seconds ahead, so those screens are read from the game alone and not what's on top of it.
On the screen copy the app serves that by the window copy (PrintWindow): no window capture, so no yellow border, at
most 12 copies a second (35 ms each at 4K, measured 17 Nov), the screen copy staying open for after. Only where that
copy comes out black while the screen shows a picture does the app open window capture, border and all, for those
moments. A script (aLogs has the screen: the Sim's end and its Gizmo tour, the automatic Battle Data session, Egg
Heist's hold) also keeps "game fast until" (1800, unix ms) a second ahead: its burst goes by window capture, quicker a
picture, with the border for those seconds, the window copy serving until window capture has its first picture.
Windows 11 always uses window capture.

A reader can also ask the graphics card to shrink its area first (picture slot byte 60: 2 or 4; byte 61 says what
was used): the card averages it down through mip levels and only the smaller picture leaves it (alogs-capture's
`read_area`). B16's damage-number recorder used it to keep 60 frames a second of a 4K screen's middle at 1080p size;
that recorder wasn't ported and is dropped for now.

Each reader's picture is written at its own pace into its own buffer, so a fast small reader never interrupts a slow
big one. Field offsets are listed at the top of `capture.rs`.

## Testing
`cargo test --bin alogs-app capture::tests::meter_reads_asked_area -- --exact --ignored --nocapture` serves Desktop
Duplication of the window in front to the meter's real reader next to a fast 60/s corner reader.
`cargo test --bin alogs-app gpu_capture::tests::duplication_speed -- --exact --ignored --nocapture` times reads
off the graphics card.
