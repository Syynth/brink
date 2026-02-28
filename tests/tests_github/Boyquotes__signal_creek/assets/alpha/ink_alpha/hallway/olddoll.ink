== olddoll ==

    {
        - currentWorld == "dream" && olddoll_real:
            -> olddoll_realvisited
            
        - currentWorld == "dream":
            -> olddoll_dream
            
        - currentWorld == "real" && olddoll_dream:
            -> olddoll_dreamvisited
        
        - else:
            -> olddoll_real
    }
//---------------- DREAM WORLD ----------------

== olddoll_dream ==

    The cracks in the doll started to disappear and the dull porcelain began to glitter. Ms Suwan reaches out to pick it up.
        
    Ms Suwan's fingertips lightly touch the glossy figurine. As she tries to grip her plam around the doll...
    
    &it quickly fades away.
    
    -> END


== olddoll_realvisited ==

    The doll lays on the floor once again...
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== olddoll_real ==

    A porcelain doll sits on a dusty shelf. Ms Suwan slowly walks around it but its glass eyes follow her. She wants to pick it up, hold it, and observe it further.
    
    NOUR: "I hate these kinds of dolls. It's eyes keep following me even when I move around its so creepy."
    
    :MS SUWAN:
    
        * [Stare longingly] I always wanted a doll like this when I was a kid. They were very popular where I grew up and almost all of my friends had them. //who is talking here?
        
    NICK: "Why didn't you have one?"
    
    :MS SUWAN:
    
        ** [Laugh softly] My mother did not allow me to have one.
        
     She continues to stare longingly at the doll. She was it to be hers but she can't bring herself to allow that pleasure.
     
     :MS SUWAN:
     
        ***Pick up the doll -> pickedUpDoll //plane switching
        
        ***Walk away ->WalkAway
        
    =pickedUpDoll
    
        &Ms. Suwan picks up the doll.
        
        -> END


==WalkAway==
    
    She breaks the eye contact between herself and the doll and brings herself to walk away
    
    ->END


== olddoll_dreamvisited ==

    The creepy old doll's eyes are following you once more. //who?
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
