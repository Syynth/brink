== sailormoonposters ==

    {
        - currentWorld == "dream" && sailormoonposters_real:
            -> sailormoonposters_realvisited
            
        - currentWorld == "dream":
            -> sailormoonposters_dream
            
        - currentWorld == "real" && sailormoonposters_dream:
            -> sailormoonposters_dreamvisited
        
        - else:
            -> sailormoonposters_real
    }
//---------------- DREAM WORLD ----------------

== sailormoonposters_dream ==

    NOUR: No way! Finally, some Sailor Moon merchandise with Uranus and Neptune!
    
    -> sailormoonposters_gender
    
    //visiting this one prompts different messages in womens clothing and cool shirt
    =sailormoonposters_gender
    
        NOUR: I love when anime characters subvert gender roles.
        
        NOUR: And when Uranus would wear a suit, man, I thought she looked so cool...
        
        NOUR: I fantasized about being like her.
    
        -> sailormoonposters_nickanime


== sailormoonposters_realvisited ==
    
    NOUR: Yeah, that's more like it! Uranus and Neptune together.
    
    -> sailormoonposters_dream.sailormoonposters_gender
    
    
//Prompts nick to look for sailor moon manga in b&n bookstack
== sailormoonposters_nickanime ==

    NICK: Woah, they're like manga characters! They look super cool!
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE

//---------------- REAL WORLD ----------------

== sailormoonposters_real ==

    NOUR: This Sailor Moon poster is great and all, but... I'd love to see more Sailor Uranus merchandise.
    
    -> END


== sailormoonposters_dreamvisited ==
    
    NOUR: Ah, it's just Sailor Moon again.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
