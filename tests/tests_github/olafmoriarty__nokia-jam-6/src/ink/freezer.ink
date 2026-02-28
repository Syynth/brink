== freezer

<-inv( ->freezer )

+ [LOOK]
It's a large walk-in freezer.
Shelves along the walls were full of boxes of ice cream.
I noticed a box on the top shelf that looked a bit different from the others, but I had no way of reaching it.
On the floor I saw the tape outline of Snow's body.

+ [LOOK AT BOX]
The box on the top shelf looked a bit different from the others. It looked older, and at the same time more solid, as if it held something more... special than the rest. It looked like it still contained ice cream, though.

+ [GET BOX]
I had no way of reaching it.

+ [LOOK AT OUTLINE]
This was where Nick Snow froze to death. No doubt about it.
{Knowledge !? murderer: 
    Who could have done such a thing?
- else:
    I knew who the murderer was. Now I just had to catch him before freezing to death myself!
}

+ [LOOK AT DOOR]
{Knowledge ? freezerLocked:
    The door was locked. I had to find a way to open it.
- else:
    The solid door to the freezer was lockable. That's what killed Nick Snow.
    Fortunately, it was open now.
}

+ [LEAVE\|GO TO KITCHEN]
{Knowledge !? freezerLocked:
I hurried back to the kitchen.
->kitchen
- else:
The door was locked.
}

+ {Knowledge ? freezerLocked && Inventory ? (napkin, soap)} [USE NAPKIN WITH SOAP\|USE SOAP WITH NAPKIN]
Wrapping the soap won't get me out of here!

+ {Knowledge ? freezerLocked && Inventory ? (napkin, water)} [USE NAPKIN WITH WATER\|USE WATER WITH NAPKIN]
I soaked the napkin in water. It froze immediately.
Before it froze to my fingers, I dropped it, and it shattered when it fell to the floor.
~ Inventory -= napkin
That's right, Aspen said there was some chemical in this water that made it super cold.

+ {Knowledge ? freezerLocked && Inventory ? (water, soap)} [USE WATER WITH SOAP\|USE SOAP WITH WATER]
I pulled out the soap, filled the outline with water from my water bottle, and it froze immediately.
I now had a key made of ice. Hopefully, it would get me out of here. #sound
~ Inventory += key

+ {Inventory ? key} [UNLOCK DOOR\|OPEN DOOR\|USE KEY]
I unlocked the door and rushed through the kitchen and into the restaurant.
->ending

- ->freezer