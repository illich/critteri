
Game inspired by:
- Oxygen Not Included
- Factorio


Core principles:

1. WYWIWYG
Everything is visible, no switching between layers.
There are no invisible features, such as gases or temperature.
Some things may have parameters (such as age of critters or growth of plants), but they are not essential to know and are hinted by graphics.

2. Family of characters
Not one controlled character (like in Factorio), not a bunch of uncontrolled characters (like ONI).
Small group of characters - one in initial level, than 2-5 in later levels.
One controlled at a time by player directly, others autonomous. Switching anytime.

3. Living world
Focus on animals and plants.
Less focus on buildings and crafting.
Boring stuff like connecting electricity ignored or automated.

4. Smaller scope
Probably like 2x3 display sizes for a level.
And 2-5 hours per level.

5. Flowing liquids
Flow would be important.
Not sure how it will work yet :)


Design

We will start with a 2D design, but later we might switch to "2.5D".
Side view.
On a 16:9 monitor, around 60 tiles (wide) are visible 
Cartoon, cute style, not really realistic.



Environment

The world is made of tiles.
Each tile has either one natural material, occuping the entire tile.
Or it can be empty (no natural material) and in that case, play can build on that tile.
Player and critters can move only on empty tiles.

There is a second layer - halftiles. Their dimension is half the size of tile, so they fit as 2x2 into a tile.
Items occupy a halftile. There can be only one item per halftile.
Liquids also occupy a halftile. They are little bit more complicated, to be described in the future.



Tech

Probably Rust, Bevy.



