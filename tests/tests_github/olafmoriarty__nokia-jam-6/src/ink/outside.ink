== outside

<-inv( ->outside )

+ [LOOK]
Snow's was located on a quiet street downtown.
The air was chilly, and the February streets were icy.
A trashcan hang next to the large door.

+ [LOOK AT DOOR]
The large double door leading inside Snow's Ice Cream Parlor had frosted glass, as if to hide the tragedy that had taken place inside.
Yellow police tape marked "DO NOT CROSS" covered the door.

+ [LOOK AT TRASHCAN\|LOOK AT TRASH]
The trashcan contained an assortment of used plastic spoons.

+ [LOOK AT GROUND\|LOOK AT STREET]
I watched where I was putting my feet carefully.
I didn't want to slip and fall here.
I'm way too young to break a hip.
The ground was littered with trash.

+ [ENTER\|ENTER BUILDING\|WALK INSIDE]
{restaurant:
    I entered the restaurant.
    ->restaurant
}
I pulled the doorknob, but the door was locked.

+ [OPEN DOOR\|UNLOCK DOOR]
{restaurant:
    I entered the restaurant.
    ->restaurant
}
The door is locked. You don't have the key.

+ [KNOCK DOOR]
{restaurant:
    No need to knock anymore. I walked inside.
    ->restaurant
}
I knocked the door.
A few seconds passed.
I knocked again.
There. I heard movement inside.
Soon, a young cop opened the door.
"It's clo... Ah. You're Detective Addison Barsk?"
"Yup. That's me."
"Good to see you. Please come inside."
I entered the restaurant. #sound
->restaurant

- ->outside
