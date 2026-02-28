== inv( ->back )
+ [INVENTORY]
{Inventory == ():
    My pockets are empty.
    ->back
}
In my pockets, I have
{Inventory ? napkin: <> a napkin,}
{Inventory ? photos: <> some photographs,}
{Inventory ? water: <> a water bottle,}
{Inventory ? soap: <> a bar of soap,}
{Inventory ? key: <> an ice key,}
<> and some pocket lint.

+ {Inventory ? napkin} [LOOK AT NAPKIN]
The napkins of Snow's Ice Cream Parlor are pink and light green, with the restaurant's logo on them.

+ {Inventory ? napkin} [USE NAPKIN]
I was pretty sure there was absolutely nothing useful I could do with this napkin.

+ {Inventory ? photos} [LOOK AT PHOTO\|LOOK AT PHOTOS\|LOOK AT PHOTOGRAPH\|LOOK AT PHOTOGRAPHS]
The photos show Crystal Snow, and a man I've never seen before, kissing intimately.

+ {Inventory ? water} [LOOK AT WATER\|LOOK AT WATER BOTTLE\|LOOK AT BOTTLE]
It was a bottle of vitamin water that tasted like cauliflower and kept cold in any temperature.

+ {Inventory ? water} [DRINK WATER\|USE WATER\|USE WATER BOTTLE\|USE BOTTLE]
This water tasted like cauliflower. I was in no rush to drink it.

+ {Inventory ? soap} [LOOK AT SOAP]
The bar of soap had the impression of the freezer key in it.

+ {Inventory ? soap} [USE SOAP]
For what?

+ {Inventory ? key} [LOOK AT KEY\|LOOK AT ICE KEY]
An exact replica of the key to the freezer, made from ice.

- ->back