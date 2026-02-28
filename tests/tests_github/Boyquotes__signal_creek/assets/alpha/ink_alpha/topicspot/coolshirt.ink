== coolshirt ==

    {
        - currentWorld == "dream" && coolshirt_real:
            -> coolshirt_realvisited
            
        - currentWorld == "dream":
            -> coolshirt_dream
            
        - currentWorld == "real" && coolshirt_dream:
            -> coolshirt_dreamvisited
        
        - else:
            -> coolshirt_real
    }
//---------------- DREAM WORLD ----------------

== coolshirt_dream ==

    NICK: I love the cute little guys on this shirt... ahh, this is totally the kind of thing I'd love to wear all the time...
    
    NOUR: Why don't you, then?
    
    NICK: I -- I don't know, actually...
    
    -> END


== coolshirt_realvisited ==

    NICK: Whoa, these shirts completely changed. Didn't they used to be kind of ugly-- I mean... like all dark and edgy before?
    
    NOUR: Yup. With this gnarly wolf pattern, too.
    
    NICK: Whoa! This one's got little birds on it now instead! "So Eggs-citing" -- pfft. That's really cute.
    
    MS SUWAN: You should get it. I'm sure it would make you very happy.
    
    :NICK:
    
    + [Take the shirt.]Nick reaches for the shirt and takes it from the rack.
        -> manager_convo
    
    + [Leave the shirt.]Nick shakes his head and steps away, leaving the shirt be.
        -> END

//SUBSEQUENT DREAM KNOTS HERE

== manager_convo ==

    NICK: Oh, right. I guess I have to talk to the manager about buying this, now... 
    
    NOUR: You sound pretty reluctant about that.
    
    NICK: Haha... it's not like I hate him or anything. It's... um, well, my mom would say buying anything here is a waste of money. 'Cause I don't really need any of this stuff, right?
    
    Nick takes a deep breath, looking down at the shirt clutched in his hands. He feels guilty about it, but suddenly feels a rush of irritation at his own guilt. How long had it been since he had last bought himself something?
    
    NICK: ...but Ms. Suwan is right. It would make me really happy... I'll talk to him anyway!
    
    -> END


//---------------- REAL WORLD ----------------

== coolshirt_real ==

    NICK: Oh, wow... this is the type of shirt that my friends would really like. Maybe... maybe I should get it...
    
    NOUR: Pfft. Doesn't seem like this type of thing suits you at all.
    
    NICK: It doesn't?! I mean... it should... 
    
    Nick doesn't sound convincing, even to himself. 
    
    -> END


== coolshirt_dreamvisited ==

    NICK: Wait, what happened to the design? It got all... edgy again.
    
    MS SUWAN: It's a shame. You seemed to like the other one better.
    
    NICK: Huh? No... of course not. I mean, this one is cool, too. The wolf design is like... so sick, or something.
    
    MS SUWAN: That was the most forced compliment I have ever heard, and I've heard a lot of forced compliments in my time. 
    
    NICK: Ugh... fine. I guess I liked the other one better. It's just that it's -- I dunno... it's so...
    
    MS SUWAN: Look, Nick. I would be a hypocrite if I judged you for liking the things that you like. It may seem like a big deal right now, but I promise that if you let yourself enjoy things without caring about what other people think, you'll feel much more content.
    
    NICK: ...I know it's such a little thing. But sometimes my friends blow stuff like this up into a big thing, and they can go for weeks talking about it...
    
    MS SUWAN: Trust me. I've been in your place before. I've had a lot of years to realize that living life becomes much easier after you let yourself be yourself.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
