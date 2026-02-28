== partyposter ==

    {
        - currentWorld == "dream" && partyposter_real:
            -> partyposter_realvisited
            
        - currentWorld == "dream":
            -> partyposter_dream
            
        - currentWorld == "real" && partyposter_dream:
            -> partyposter_dreamvisited
        
        - else:
            -> partyposter_real
    }
//---------------- DREAM WORLD ----------------

== partyposter_dream ==

    NICK: Oh, wow. Are these posters for a party or something? Whoa, free food and drinks...! Hey, we should all go together!
    
    NOUR: Ah, I'm not much of a party person...
    
    NICK: Huh? Why not?
    
    NOUR: Just not my thing. I went to a couple in college, and you know, been to one, been to 'em all.
    
    NICK: Oh... I've always had fun in the parties that my school throws. I mean, free pizza and soda, right? Ms. Suwan, what about you? Were you a big partier?
    
    MS SUWAN: Not particularly.
    
    +   { phonecases_dream } -> phoneconvo
    
    + -> END


== partyposter_realvisited ==

    NICK: Back to the party again... you still think we can go?
    
    -> END


== phoneconvo ==

    NICK: Ms. Suwan, are you sure you don't wanna go to the party?
    
    MS SUWAN: I'll go on one condition.
    
    NICK: What is it?
    
    MS SUWAN: You let me buy something for you.
    
    NICK: Oh... uh... I'll... I'll think about it.
    
    -> END

//---------------- REAL WORLD ----------------

== partyposter_real ==

    NICK: What is this for? I can hardly even read it... lame.
    
    MS SUWAN: It seems like it was originally for a party of sorts...
    
    NOUR: Ha. The date says sometime three years ago.

    -> END


== partyposter_dreamvisited ==

    NICK: Oh, what happened to the poster? It's all... torn and worn out. I can't even read the details anymore... Does this mean the party's cancelled...? What a bummer.
    
    -> END


