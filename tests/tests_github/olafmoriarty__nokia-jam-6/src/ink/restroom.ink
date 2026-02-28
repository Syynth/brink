== restroom

<-inv( ->freezer )

+ [LOOK]
The unisex restroom had six stalls.
Each stall had a toilet in it.
By the door, there were two sinks underneath a large mirror.
Nothing looked out of the ordinary.

+ [LOOK AT STALLS\|LOOK AT STALL]
They were ordinary restroom stalls.

+ [LOOK AT MIRROR]
I caught my reflection in the mirror. I guess that means the mirror works.

+ [LOOK AT SINKS\|LOOK AT SINK]
The white sinks looked like they were great for washing your hands.

+ [USE SINK\|WASH HANDS]
I quickly washed my hands, and turned the faucet off agin.

+ [USE TOILET\|USE RESTROOM\|USE BATHROOM\|PEE\|POO\|ENTER STALL]
Maybe later. Not while I'm working.

* (mirrorshake) {Knowledge ? mirror} [SHAKE MIRROR]
This felt silly, but...
I reached out, touched the mirror, and shaked it a bit.
From behind the mirror, two photographs suddenly fell to the floor. #sound

+ {mirrorshake && Inventory !? photos} [LOOK AT PHOTOS\|LOOK AT PHOTO\|LOOK AT PHOTOGRAPHS\|LOOK AT PHOTOGRAPH]
I should probably pick them up first.

+ {mirrorshake && Inventory !? photos} [TAKE PHOTOS\|TAKE PHOTO\|TAKE PHOTOGRAPHS\|TAKE PHOTOGRAPH]
I picked up the photos from the floor and looked through them. #sound
~ Inventory += photos
I'm guessing Aspen took these.
The photos show Crystal Snow, and a man I've never seen before.
They meet on the street outside the restaurant, and they kiss. Intimately.
I was pretty sure this was not her husband.
I had to talk to Crystal about this.

+ [LEAVE\|GO TO RESTAURANT\|GO OUT]
I quietly left the restroom again.
->restaurant

- ->restroom