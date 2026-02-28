VAR currentPartyChar = "THE PARTY"
VAR currentPlane = "REAL"
VAR hasShirt = false

//TODO: make hasshirt work

It's a clothing rack, full of bright, colorful t-shirts all pertaining to today's warmest trends.
{currentPartyChar == "NICK":
    -> NickTShirtDreamRack
}
    -> NoirSuwanTShirtDreamRack

=== NickTShirtDreamRack ===
They still call to Nick, his vision swimming with the patterns of their designs.

:NICK:
* [ Reach out for another shirt ] -> TakeShirtDream
* [ Pull Away ] -> PullAwaySuccessful

=== TakeShirtDream ===
// boolean - hasshirt is false
Nick takes another shirt, grabbing for its incoporeal form, but his hand seems to pass through it. He can't get a hold of it, none of the shirts want to be his. He has the one and only.
& //shift plane
-> END

=== PullAwaySuccessful
He ignores them, stepping away from the shirt rack with his prize still in hand. 
-> END

=== NoirSuwanTShirtDreamRack ===
It's a clothing rack, full of bright, colorful t-shirts. Their colors seem to swim in front of everyone's eyes as they look at them.  
-> END
