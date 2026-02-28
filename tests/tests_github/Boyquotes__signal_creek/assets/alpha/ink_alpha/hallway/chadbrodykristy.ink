== chadbrodykristy ==

    {
        - currentWorld == "dream" && chadbrodykristy_real:
            -> chadbrodykristy_realvisited
            
        - currentWorld == "dream":
            -> chadbrodykristy_dream
            
        - currentWorld == "real" && chadbrodykristy_dream:
            -> chadbrodykristy_dreamvisited
        
        - else:
            -> chadbrodykristy_real
    }
//---------------- DREAM WORLD ----------------

== chadbrodykristy_dream ==

    Ms Suwan stands in shock as she sees a unwelcomimg familiar face.
    
    CHAD: Hey Kannika, what are you doing here? I thought you were studying for the LSAT.
    
    KANNIKA: I am, but I needed to take a break and do some shopping.
    
    Ms Suwan begins starts shaking with embarrassment but quickly forces herself to stop.
    
    BRODY: Oh, I see. Well, I hope you didn't spend too much money on frivolous things. You know the LSAT is a very important test and you need to focus on studying.
    
    KANNIKA: I didn't spend too much and I am still focused on studying.
    
    KRISTY: Yeah, but shopping can be distracting. You need to be disciplined and stay away from temptations like that.
    
    KANNIKA: I am disciplined and I can handle a little shopping break.
    
    CHAD: Well, we'll see how well you do on the LSAT. Maybe next time you'll prioritize studying over shopping.
    
    KANNIKA: Thanks for the support guys. I'll make sure to keep that in mind.
    
    CHAD: I mean, I heard you failed the last exam.
    
    Chad turns to Brody and Kristy and begins to snicker.
    
    BRODY: Yeah, that's what I heard too.
    
    KRISTY: What happened? Did you not study enough?
    
    KANNIKA: Actually, I was sick during the exam and couldn't focus.
    
    Ms Suwan is visibly starting to look frustrated.
    CHAD: Oh, that's too bad. But you know, maybe you should try harder next time.
    
    BRODY: Yeah, failing an exam is never a good look for a law student.
    
    KRISTY: Maybe you should consider finding a different career path if you can't handle the pressure.
    
    KANNIKA: I'll keep that also in mind.
    
    CHAD: No problem. We're just looking out for you.
    
    BRODY: Yeah, we don't want you to embarrass yourself any further.
    
    KRISTY: And we definitely don't want you to bring down the reputation of our law school.
    
    KANNIKA: I appreciate your concern, but I think I can handle it.
    
    CHAD: Well, we'll just have to see about that. Good luck on the next exam.
    
    BRODY: Yeah, you're going to need it.
    
    KRISTY: Let's just hope you don't fail again.
    
    CHAD: Come on guys lets go hit up Abercromb
    
    BRODY: I'll catch you later Kan, hopefully in class.
    
    KRISTY: Yea see ya Kan.
    
    Chad, Brody, and Kristy disappear while Ms Suwan stares blank eyed into the distance.

    -> END

    
== chadbrodykristy_realvisited ==

    -> chadbrodykristy_dream
    
//---------------- REAL WORLD ----------------

== chadbrodykristy_real ==
    
    -> chadbrodykristy_dream


== chadbrodykristy_dreamvisited ==

    -> chadbrodykristy_dream
