== polaroid ==

    {
        - currentWorld == "dream" && polaroid_real:
            -> polaroid_realvisited
            
        - currentWorld == "dream":
            -> polaroid_dream
            
        - currentWorld == "real" && polaroid_dream:
            -> polaroid_dreamvisited
        
        - else:
            -> polaroid_real
    }
//---------------- DREAM WORLD ----------------

== polaroid_dream ==

    //This was the first polaroid camera Nour had. Her parents bought it for her when she first started showing interest in photography, since her mother's family had one when she was young.
    
    NOUR: It's an old polaroid! Looks like it's one of the O-G's from the 80s.   
    
    NICK: It definitely seems old-school. Have you ever used one of them before?
    
    NOUR: Mmhm, I've had a ton of different film camera's. The one with me now is a film camera, too, though a more modern one. 
    
    NOUR: Wish I had one now like this one, but they tend to be pretty pricey.
    
    -> END

== polaroid_realvisited ==

    //This was the first polaroid camera Nour had. Her parents bought it for her when she first started showing interest in photography, since her mother's family had one when she was young.
    
    NOUR: Oh wow, it's the same polaroid! Looks like it's actually functional, though.
    
    NOUR: I don't really use them anymore, though I think there are some of the o-g retro ones at my university.
    
    NOUR: I wonder if there's any film around...
    
    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== polaroid_real ==

    //This was the first polaroid camera Nour had. Her parents bought it for her when she first started showing interest in photography, since her mother's family had one when she was young.
    
    NOUR: Oh man, this camera is so old. It must've been here for forever. Though this one could be new and just neglected -- social media caused a resurgence of these types of cameras.
    
    NOUR: The film in it must be completely useless by now, though.
    
    NOUR: I remember by mom giving me an old family camera that looked a lot like this one. It was my first real camera, now that I think about it...
    
    -> END


== polaroid_dreamvisited ==

    NOUR: It's the polaroid again. This one looks pretty f-ed up, though.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
