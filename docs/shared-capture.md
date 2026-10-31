# Shared game capture

The aLogs app captures the game once for every module (the meter, Sanctum Helper, later ones) and shares the
pictures through the named shared memory `Local\aLogs.Capture.v2`. Writer: `app/src-tauri/src/capture.rs` (with
`gpu_capture.rs`). Readers: `bossbar/wincapture.py` (`SharedCapture`) and
`SanctumHelper/app/SanctumHelper/SharedCapture.cs`.

## How pictures are taken
Both ways keep the picture on the graphics card and copy into memory only the areas readers ask for:

1. **Window capture** (Windows Graphics Capture), only where the yellow border can be turned off (Windows 11).
   Just the game window, so our overlays are never in it.
2. **Desktop Duplication** of the screen the game is on, only while the game is in front. Our overlays can be in
   it (flags bit 1): the meter and Sanctum Helper hide their windows from capture then.

PrintWindow and BitBlt screen copies (v1) were removed: ~50 ms per 4K frame, and they made the game wait.

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
parts and sends the box around them, so the reader handles both (bossbar/wincapture.py `read_rects`). The meter uses
this for screen reads between fights: the top band plus the screens' text boxes, about half of a 4K frame.

A reader can also ask the graphics card to shrink its area first (picture slot byte 60: 2 or 4; byte 61 says what
was used): the card averages it down through mip levels and only the smaller picture leaves it. The damage-number
recorder (bossbar/dmgnumbers.py) uses this to keep 60 frames a second of a 4K screen's middle at 1080p size.

Each reader's picture is written at its own pace into its own buffer, so a fast small reader never interrupts a slow
big one. Field offsets are listed at the top of `capture.rs`.

## Testing
`cargo test --bin alogs-app capture::tests::meter_reads_asked_area -- --exact --ignored --nocapture` serves Desktop
Duplication of the window in front to the meter's real reader next to a fast 60/s corner reader.
`cargo test --bin alogs-app gpu_capture::tests::duplication_speed -- --exact --ignored --nocapture` times reads
off the graphics card.
