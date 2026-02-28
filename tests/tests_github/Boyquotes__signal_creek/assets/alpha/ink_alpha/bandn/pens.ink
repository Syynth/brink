== pens ==

    {
        - currentWorld == "dream" && pens_real:
            -> pens_realvisited
            
        - currentWorld == "dream":
            -> pens_dream
            
        - currentWorld == "real" && pens_dream:
            -> pens_dreamvisited
        
        - else:
            -> pens_real
    }
//---------------- DREAM WORLD ----------------

== pens_dream ==

    NICK: Oh my god! Look at these cute pens! I really, really want one... look at how cute they are! Look at the frog one, it's so cute... 
    
    The frog perched on top of the pen seems to give him a little wink, which makes Nick very happy. He reaches over and takes the pen.
    
    NICK: I bet if I did my homework with this I'd never get anything wrong!
    
    -> END


== pens_realvisited ==

    NICK: Is it just me or did these pens get even cuter?
    
    The frog topper on the pen he had been eying earlier seems to give him a little wink. Instantly, Nick is endeared, reaching over to pick it up. Any earlier reservations he had seem to fade away.
    
    NICK: You know what, Ms. Suwan - you were right. Definitely. These are so cute that it doesn't matter if they're just for babies! As long as they make me happy, right? And they do make me happy! Just look at that cute little frog!
    
    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== pens_real ==

    NICK: These pens... Haha, I like how bendy they are. Oh my god. Look at how cute the frog on that one is!
    
    He seems to look at them wistfully before tearing his gaze away.
    
    NICK: I - I don't WANT them, though. That stuff's for kindergarteners.
    
    MS SUWAN: You've been looking at it awfully long for someone who doesn't want it.
    
    NICK: Huh? But -- I really don't actually want it, though... I mean, it's way too expensive for a pen. I could get a regular one for free if I looked on the floor of my school for like five minutes... 
    
    MS SUWAN: You're still looking at the frog pen, Nick.
    
    NICK: No, I'm not, I'm just... I dunno... I mean, a pen for babies is definitely not worth four whole dollars...
    
    MS SUWAN: Anything can be worth any amount if it makes you happy enough, Nick.
    
    NICK: ... I guess so. But... still, I'm not gonna get it.
    
    -> END


== pens_dreamvisited ==

    NICK: Ah, they're not as cute anymore, but... they're still a little cute. Guess it doesn't change the fact these are for babies, though.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
