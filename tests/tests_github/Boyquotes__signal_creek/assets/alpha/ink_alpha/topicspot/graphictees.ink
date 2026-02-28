== graphictees ==

    {
        - currentWorld == "dream" && graphictees_real:
            -> graphictees_realvisited
            
        - currentWorld == "dream":
            -> graphictees_dream
            
        - currentWorld == "real" && graphictees_dream:
            -> graphictees_dreamvisited
        
        - else:
            -> graphictees_real
    }
//---------------- DREAM WORLD ----------------

== graphictees_dream ==
    
    The rack of tees are covered with graphic iconography and strong slogans, like pride flags in eye-catching patterns. One of them proudly calls to "legalize weed". Some of them are purposefully holey. Even so, they are all designed appealingly. 
    
    MS SUWAN: I can't see myself wearing any of these.
    
    Even so, there's an amused glint in her eye as she continues to peruse the rack.
    
    MS SUWAN: I suppose it all comes from my teenage years... That's when I developed a habit of choosing safety in dull clothes.
    
    NICK: Y'know, I think they're kinda cool!
    
    MS SUWAN: Cool, maybe. Bold, definitely.
    
    -> END

== graphictees_realvisited ==

    +   { hotwheels_dream } -> hotwheelsconvo
    
    The rack of tees are covered with graphic iconography and strong slogans, like pride flags in eye-catching patterns. One of them proudly calls to "legalize weed". Some of them are purposefully holey. Even so, they are all designed appealingly. 
    
    MS SUWAN: Ah... they look a little better, here. Still, I can't see myself wearing any of these.
    
    There's an amused glint in her eye as she continues to peruse the changes in the clothing.
    
    MS SUWAN: I suppose it all comes from my teenage years... That's when I developed a habit of choosing safety in dull clothes.
    
    NICK: Y'know, I think they're kinda cool!
    
    MS SUWAN: Cool, maybe. Bold, definitely.
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== graphictees_real ==
    
    The rack of tees are covered with over-the-top graphic iconography and strong slogans, like pride flags. One of them proudly calls to "legalize weed". They are purposefully ripped and holey.
    
    MS SUWAN: Little tacky, aren't they...
    
    NICK: Y'know, I think they're kinda cool!
    
    MS SUWAN: Let's agree to disagree.
    
    -> END


== graphictees_dreamvisited ==
    
    The rack of tees are covered with over-the-top graphic iconography and strong slogans, like pride flags. One of them proudly calls to "legalize weed". They are purposefully ripped and holey.
    
    MS SUWAN: I like these a bit less than before... I supposed they've changed.
    
    NICK: I still think some of them are kinda cool...
    
    MS SUWAN: ...let's agree to disagree.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE

//---------------- HOT WHEELS CONNECTION ----------------

== hotwheelsconvo ==
//Not sure how to format objects talking to each other yet?
//After hot wheels conversation/connection, and dream plan visited, they are in real plan discussing the shirts again.

    The rack of tees are covered with over-the-top graphic iconography and strong slogans, like pride flags. One of them proudly calls to "legalize weed". They are purposefully ripped and holey.
    
    MS SUWAN: I like these a bit less than I did before. Did they change, too?
    
    NICK: I mean, I dunno. I still think some of them are kinda cool.
    
    MS SUWAN: ...let's agree to disagree.
    
    NICK: Haha. All right. I guess we can't see eye-to-eye about everything like we did with the hot wheels.
    
    Nick shuffles through the rack and takes out one of the shirts.
    
    NICK: But look! This one isn't that bad, right? Don't you think the lightning bolt is stylish?
    
    Despite herself, Ms. Suwan smiles.
    
    MS SUWAN: Still not something I'd wear, but... it is a little stylish, I suppose.
    
    -> END
