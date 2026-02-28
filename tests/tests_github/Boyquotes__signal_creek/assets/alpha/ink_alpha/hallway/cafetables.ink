== cafetables ==

    {
        - currentWorld == "dream" && cafetables_real:
            -> cafetables_realvisited
            
        - currentWorld == "dream":
            -> cafetables_dream
            
        - currentWorld == "real" && cafetables_dream:
            -> cafetables_dreamvisited
        
        - else:
            -> cafetables_real
    }
//---------------- DREAM WORLD ----------------

== cafetables_dream ==

    The tables are clean and shiny, ready for people to sit at them and eat. Napkins and sauces stand at the ready.
    
    NICK: Oh! Oh there's my favorite seat! I remember so many good times there, the sauces were always so tasty, and we didn't have to ask to use them!
    
    NOUR: We? 
    
    NICK: Yeah! Me and my friends used to hang out here a lot. We got nachos with cheese that we could all share or hot dogs or shit! 
    
    Nour seems to look over the tables, suddenly thoughtful. A frown is on her face.
    
    NOUR: I have memories here too, actually.
    
    NICK: Really?! Hey, wanna share stories?
    
    NOUR: Not right now.

    -> END


== cafetables_realvisited ==

    The tables seem even shinier, now that the dirt of the other world is gone. Nick looks at all of them fondly, this time not just at his favorite.
    
    NICK: I like it better this way... I don't like seeing the tables so run down. Someone cares for them, here. Strange. Maybe this *isn't* the backrooms but some... some *good* version of the backrooms, you know?
    
    NOUR: Maybe its not the backrooms at all...
    
    NICK: Well I'm going to keep calling them the backrooms. Or maybe if not the backrooms then the... the frontrooms. The rooms on the good side where things aren't creepy but in fact really really cool.
    
    MS SUWAN: Well I think we should move on before I start to hallucinate these backrooms too. 
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== cafetables_real ==

    The tables are dirty and run down. There's gum stuck to the bottom of all of them to various degrees.
    
    NICK: ... Gross. I thought the cafe was the most important place, how could they let it become like this?
    
    NOUR: I don't think there was enough business to warrant keeping the food places open. The vending machines are probably more than enough and cheaper.
    
    NICK: Yeah but... the vending machines don't have the same energy, you know? Vending machines are boring.
    
    -> END


== cafetables_dreamvisited ==

    The tables seem in an even sadder state, now. Nick tries to pick at some of the gum with a knife.
    
    NICK: Maybe... maybe with enough work we can get at least *one* table into good condition, like in the other place, right? 
    
    He keeps picking at it but eventually sighs and gives up.
    
    NICK: Nevermind... there's no free sauces here anyways, and that's the best part.
    
    Ms.Suwan puts a hand on his shoulder, offering him a brief, sad smile.
    
    MS SUWAN: It's alright, Nick. You can think back on the good times, you don't need these tables specifically.
    
    NICK: Yeah... I guess you're right. I'm still going to miss them, though. I wish they went out with a bang, not like this.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
