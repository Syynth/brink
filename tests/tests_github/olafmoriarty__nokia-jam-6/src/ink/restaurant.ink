== restaurant

{(dishwasher && wife) && Knowledge !? chefishere:
    As I entered the restaurant, a woman came in.
    "Who are you?"
    "I'm Aspen. The chef. You're cops?"
    "Glad you could come."
    "You need to talk to me?"
    "That would be great. Why don't you sit down? I'll be with you shortly."
    ~ Knowledge += chefishere
    She sat down at a table and started scrolling social media on her phone.
}
->options

= options
<-inv( ->restaurant )
+ [LOOK]
The room I had entered was a large restaurant space.
Chairs and tables were neatly placed everywhere.
Against the back wall was a counter.
I saw a door leading to the kitchen behind the counter.
A sign above me pointed the way to the restrooms.
In the back of the room, a stairway led to a door marked "PRIVATE".

+ [LOOK AT TABLES\|LOOK AT TABLE]
The tables were all spotlessly clean.
Somebody must have cleaned them last night, after closing time.

+ [LOOK AT CHAIR\|LOOK AT CHAIRS]
I saw nothing irregular about the chairs.
They looked expensive for an ice cream place.
I guess that was to be expected here.

+ [SIT\|SIT DOWN]
I don't have time to rest now.
I have a murder to solve!

+ [TALK TO COP]
->cop

+ {dishwasher && wife} [TALK TO ASPEN\|TALK TO CHEF\|TALK TO WOMAN]
->chef

+ (counter) [LOOK AT COUNTER]
The counter is empty except for a napkin holder and a cash register.

+ {counter} [LOOK AT REGISTER\|LOOK AT CASH REGISTER\|USE REGISTER\|USE CASH REGISTER]
I pushed a button on the cash register, and the drawer popped out.
So there was probably no burglary here.
That would have been too simple.
I closed up the cash register again.

+ {counter} [LOOK AT NAPKIN\|LOOK AT NAPKINS\|LOOK AT NAPKIN HOLDER]
The napkins of Snow's Ice Cream Parlor are pink and light green, with the restaurant's logo on them.
{Inventory !? napkin: I could probably take one.}

+ {counter} [TAKE NAPKIN]
{Inventory ? napkin:
    I already have one.
- else:
    I grabbed a napkin and put it in my pocket. #sound
    ~ Inventory += napkin
}

+ [LOOK AT SIGN]
The letters "WC" were printed on the sign hanging from the ceiling.
It points towards a door in the back of the room.

+ {dishwasher && wife} [LOOK AT CHEF\|LOOK AT ASPEN]
The chef was a woman in her late thirties.
She sat by a table, scrolling her phone.

+ [GO OUT\|GO OUTSIDE\|LEAVE]
I left the restaurant.
->outside

+ [GO TO KITCHEN]
I strolled into the kitchen.
->kitchen

+ [GO TO RESTROOM\|GO TO TOILET\|GO TO BATHROOM\|GO TO RESTROOMS\|GO TO TOILETS\|GO TO BATHROOMS]
I opened the door to the restroom and walked inside.
->restroom

+ [GO UPSTAIRS\|GO STAIRS\|GO UP\|CLIMB STAIRS]
I climbed the stairs up to Snow's private apartment, and opened the door.
->apartment

- ->restaurant