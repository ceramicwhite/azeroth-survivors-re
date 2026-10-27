# Controls

**Azeroth Survivors** — top-down, one hand on WASD, the other on the wheel.

| When | Key / Input | Action |
|---|---|---|
| Menu | Click a race, gender, class and zone | make your pick (**Randomize** rolls one) |
| Menu | **Enter** (or **Enter World**) | start the run; the world travels to the zone picked |
| Playing | **W / S / A / D** or **arrow keys** | move the hero (locked camera: the hero faces the cursor, backpedals and strafes) |
| Playing | **Space** | jump |
| Playing | **F** (from level 5) | your class's movement spell: Blink, Charge… aim at the cursor |
| Playing | **C** | unlock / lock the camera |
| Any | **G** | toggle the picture polish (`?grade=0` starts it off) |
| Playing | **mouse wheel** | zoom, from over the shoulder out to 70 yd |
| Playing | **drag** (camera unlocked) | swing and tilt the camera |
| Playing | **Esc** | pause |
| Playing | — | abilities cast themselves (no aim) |
| Level up | **1 / 2 / 3** (or click a card) | pick an upgrade |
| Paused | **Esc** | resume |
| Game over | **Enter** (or **Play again**) | back to the menu |
| Any | minimap **+ / −** / minimize | minimap zoom / collapse |

The camera follows the hero, locked top-down by default (laptop-friendly). **C** unlocks it so a
mouse drag swings and tilts it; **C** again snaps it back. In the menu it slowly orbits the
preview hero. Your race, gender, class and zone are remembered across reloads.

## Controller

Any gamepad your OS (or browser) reports as a standard gamepad should work, on macOS and Windows,
native or in the page, wired or Bluetooth. Buttons are named by position (A is the bottom face
button: Cross on a PlayStation pad). Several pads can be connected;
they all drive the one hero.

| When | Pad | Action |
|---|---|---|
| Playing | **left stick** or **D-pad** | move (a half-tilt walks at half speed) |
| Playing | **right stick** (locked camera) | aim: the hero faces it and the movement spell goes that way |
| Playing | **right stick** (camera unlocked) | swing and tilt the camera |
| Playing | **A** | jump |
| Playing | **X** or **right trigger** | your class's movement spell |
| Playing | **Y** | unlock / lock the camera |
| Playing | **LB / RB** | zoom out / in |
| Any | **Back / Select** | toggle the picture polish |
| Playing / Paused | **Start** | pause / resume |
| Paused | **B** | resume |
| Menu | **D-pad** or **left stick** | move the highlight between races, classes, zones and buttons |
| Menu, level up, paused, game over | **A** | press the highlighted button or card |
| Menu | **Start** | start the run |
| Game over | **Start** | back to the menu |

The menu highlight shows only while the pad was the last thing touched; moving the mouse or
pressing a key hands the screen back to them. The first pad press after that only brings the highlight
back; press A again to confirm. A stick that rests slightly off-centre is ignored
(`DEADZONE` in `survivors/pad.rs`).

In the browser, a page only sees a pad once a button on it has been pressed, so press one after
loading. The page's audio still starts on the first click or key: a pad button is not a user
gesture to the browser, so click once if the game is silent.

Not on the pad: the minimap's zoom and minimize buttons, and the 1 / 2 / 3 card shortcuts (pick a
card by moving the highlight to it and pressing A).
