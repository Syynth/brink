== makeup ==

    {
        - currentWorld == "dream" && makeup_real:
            -> makeup_realvisited
            
        - currentWorld == "dream":
            -> makeup_dream
            
        - currentWorld == "real" && makeup_dream:
            -> makeup_dreamvisited
        
        - else:
            -> makeup_real
    }
//---------------- DREAM WORLD ----------------

== makeup_dream ==

    //On her first trip to the mall alone after getting her driver's license, Nour bought a palette of black eyeshadow from the TopicStop.
    
    NOUR: These eyeshadow palettes were such a blessing and a curse. My first brand of punk makeup.
    
    NOUR: Funny, all of the ones here are the exact same one I used. It was discontinued years ago.
    
    MS SUWAN: I doubt this is truly safe for someone's skin.
    
    NOUR: I mean, I used one of these palettes of eyeshadow for four years and my skin seems to have turned out fine.
    
    NOUR: I used to carry it around everywhere so my parents wouldn't find it. I'd put it on in the school bathroom and would take it off before I went home. Kept it hidden in my car, too. 
    
    NICK: I always thought makeup was really cool... I said that to my mom once and she got really mad. 
    
    NOUR: Boo, she shouldn't have! Makeup's cool for everyone, punk and goth makeup especially. 
    
    NOUR: It's been worn by all kinds of people since like, the seventies.
    
    NICK: Huh... Could I... try some?
    
    NOUR: Oh, sure! I'm not very good at it, but black eyeshadow can be a little messy. You can rub it on with your finger if we can't find a brush.
    
    -> END


== makeup_realvisited ==
    
    NOUR: Less generic brands here. Funny, all of these are the exact same one I use to use. It was discontinued years ago.
    
    MS SUWAN: I doubt this is truly safe for someone's skin.
    
    NOUR: I mean, I used one of these palettes of eyeshadow for four years and my skin seems to have turned out fine.
    
    NOUR: I used to carry it around everywhere so my parents wouldn't find it. I'd put it on in the school bathroom and would take it off before I went home. Kept it hidden in my car, too. 
    
    NICK: I always thought makeup was really cool... I said that to my mom once and she got really mad. 
    
    NOUR: Boo, she shouldn't have! Makeup's cool for everyone, punk and goth makeup especially. 
    
    NOUR: It's been worn by all kinds of people since like, the seventies.
    
    NICK: Huh... Could I... try some?
    
    NOUR: Oh, sure! I'm not very good at it, but black eyeshadow can be a little messy. You can rub it on with your finger if we can't find a brush.
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== makeup_real ==
    
    //On her first trip to the mall alone after getting her driver's license, Nour bought a palette of black eyeshadow from the TopicStop.
    
    NOUR: Just a generic brand of black makeup.
    
    NOUR: With how long it's probably been here, I couldn't be paid enough to put that on my skin.
    
    -> END


== makeup_dreamvisited ==

    NOUR: Looks like a newer version of the palettes we saw.
    
    NOUR: The packaging is different, but the colors look the same.
    
    NOUR: ...And of course it's tripled in price.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
