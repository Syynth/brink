# title: The Cave
# author: Matt Woelk

# hide_choices

-> start


== start

The sun hits your lips as you wake from a dream.

You mind's clouds mix with reality as you rub your eyes. Your everything is full of sand, and your hair is damp.

On your left is the ocean 🌊, in front of you is the shore 🏖, and to your right is the rocks 🗻.

* {not ocean} 🌊 -> ocean

+ 🏖 -> shore

+ 🗻 -> rocks

* 😴 -> sleep


== sleep

You fall back asleep.

-> start


== ocean

You wade into the ocean.

The surf froths against your calves, then your thighs, then your neck, then your face, then your ...

-> start


== shore

You walk along the shore.

The gulls cackle as the waves shush them. You find an orb 🔮.

The ocean 🌊 is on your left, the rocks 🗻 are on your right, and the orb 🔮 is at your feet.

* {not ocean} 🌊 -> ocean

* 🔮 -> orb_approach

+ 🗻 -> rocks


== orb_approach

You reach to pick up the orb, but ⚡! You get zapped!

The orb falls to the sand, seeming somehow different.

The ocean 🌊 is on your left, the rocks 🗻 are on your right, and the orb 🔮 is at your feet.

* {not ocean} 🌊 -> ocean

* 🔮 -> orb_again

+ 🗻 -> rocks


== orb_again

You reach for the orb 🔮 once again.

There is no shock.

You hold the orb 🔮 in your hands, and you hear a voice from within your own mind. It says:

`... no time to ... accident ... I just hope ... ...`

It sounds both close and far away.

-> orb_holding


== orb_holding

You are holding the orb.

* 👅 -> tongue

* 🌊 -> orb_to_ocean

+ 🗻 -> rocks_with_orb


== orb_to_ocean

You throw the orb 🔮 into the ocean.

You are no longer holding the orb 🔮.

You head toward the rocks.

-> rocks


== tongue

You lick the orb.

Salty.

Nothing happens.

-> orb_holding


== rocks_with_orb

You climb up the jagged rocks, clutching the orb 🔮 in your arms.

It's too heavy to bring with you. You grab it with both your hands and hear:

`... the fire ... enough time ... ...`

* 🤔 -> think_about_leaving_orb_behind


== think_about_leaving_orb_behind

You stop to think about the orb 🔮.

It's too heavy to bring, but it must stay safe. You reach up and tuck it under a gull's nest on the nearby ledge.

-> rocks


== rocks

You climb to the top of the rocks 🗻 and see a cave entrance ⚫.

+ ⚫ -> cave_intro

* 👂 -> cave_listen


== cave_listen

You listen at the entrance of the cave. You hear hundreds of fluttering bats 🦇, then a slow drip of water 💧.

-> cave_intro


== cave_intro

The cave is dimly lit from a hole in the ceiling.

The floor is covered in bat dung 💩, there is a giant cube of glass in the distance 🧊, and you can hear bats down another corridor to the right 🦇.

-> cave


== cave

You stand alone in the cave.

* 💩 -> bat_poop

* 🧊 -> cube

* 🦇 -> bats


== bat_poop

You inspect the bat dung.

It looks, smells, and tastes just like bat dung.

-> cave


== cube

You walk up to the giant glass cube 🧊 .

It is perfect, motionless, soundless, inviting.

* 🖐️ -> touch_the_cube


== back_to_cave

You go back to the entrance of the cave.

-> cave


== touch_the_cube

You touch the cube 🧊 .

⚡⚡

You feel a shock run through you from your hand straight to your brain. You keep your hands on, and hear:

`... can't hear ... still try ... ...`

You leave your hands on the cube 🧊 , listening, listening.

But the voice, which seemed to come from your own mind, has faded off.


-> cave


== bats

You walk in the direction of the bats.

🦇🦇🦇

The sound of ten thousand bats, chirping and fluttering, fills your ears.

You feel the force of twenty thousand wings push down on you from above.

The sounds fade as the bats all exit the cave, replaced by the sound of a steady drip of water 💧.

* 💧 -> drip


== drip

You follow the dripping sound.

The tunnel gets darker and darker, then lighter and lighter.

Then you see it.

Glowing, shimmering, brilliant, a gem 💎 larger than any you could imagine, hanging from the ceiling, with water 💧 slowly dripping from it.

The point at the bottom is just low enough to touch.

* 🖐️ -> touch_the_gem


== touch_the_gem

You touch the gem.

⚡⚡⚡

Flashes of light spin around you, or through you? It feels as though your brain is directly connected to the gem.

The gem goes dark. For a moment, you are in complete darkness, hand still touching the bottom of the gem 💎, frozen in space and time.

* 👁 -> wake_up


== wake_up

Your eyes open.

Everything is flashing red and blue. You breathe in the cold air.

`... waking up ... ...`

You cough. You cough a whole lot.

Someone in a big red helmet looks over you as you lie down.

`You made it out. I can't believe it. The whole place was filled with smoke. How did you find your way?`

You think for a moment.

"I don't know. I just knew, like I had a guide."

`Well I'm just glad you're okay. The fire's almost out and the ambulance is on the way.`

"Thank you, thank you" you say, as the sound of the water dousing the flames turns back into the rolling of the waves 🌊 as you drift off to sleep.


-> END































