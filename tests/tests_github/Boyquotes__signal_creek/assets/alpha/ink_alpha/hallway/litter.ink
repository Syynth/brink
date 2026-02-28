== litter ==

    {
        - currentWorld == "dream" && litter_real:
            -> litter_realvisited
            
        - currentWorld == "dream":
            -> litter_dream
            
        - currentWorld == "real" && litter_dream:
            -> litter_dreamvisited
        
        - else:
            -> litter_real
    }
//---------------- DREAM WORLD ----------------

== litter_dream ==
    
    Scattered on the ground are wrappers from candy bars and crumpled flyers
    
    NOUR: Who leaves wrappers like that, people man
    
    NICK: I can help clean it up
    
    MS SUWAN: That's very nice of you Nick
    
    Nick picks up a wrapper and takes a few steps away from the trash can. 
    
    NOUR: Nick what are you doing
    
    NICK: Trust me I got this
    He throws the wrapper toward the trash can, landing it perfectly 
    
    NICK: YES
    
    NOUR: That was pretty cool
    
    NICK: I'M GONNA TRY TO GET THE REST IN!
    
    NOUR: ok hurry up
    
    -> END


== litter_realvisited ==

    Scattered on the ground are wrappers from candy bars and crumpled flyers
    
    NOUR: Who leaves wrappers like that, people man
    
    NICK: I can help clean it up
    
    MS SUWAN: That's very nice of you Nick
    
    Nick picks up a wrapper and takes a few steps away from the trash can. 
    
    NOUR: Nick what are you doing
    
    NICK: Trust me I got this
    
    He throws the wrapper toward the trash can, landing it perfectly 
    
    NICK: YES
    
    NOUR: That was pretty cool
    
    NICK: I'M GONNA TRY TO GET THE REST IN
    
    NOUR: ok hurry up
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== litter_real ==

    Scattered on the ground are wrappers from candy bars and crumpled flyers promoting outdated cover bands. 
    
    Nick picks up the litter and throws it in the trash.
    
    -> END


== litter_dreamvisited ==

    Scattered on the ground are wrappers from candy bars and crumpled flyers promoting outdated cover bands. 
    
    NICK: Do you want to try throwing the litter in the trash can from here 
    
    MS SUWAN: I dont know, I'm not good at playing those kinds of games
    
    NICK: Come on, we're helping the environment by throwing it out we're jsut doing it in a funner way
    
    NOUR: Funner's not a word 
    
    NICK: Really?
    
    MS SUWAN: I guess I can try
     
    Ms Suwan prepares to throw the crumpled up wrapper 
    
    NICK: come on you got this
    
    NOUR: go go go 
    
    Ms Suwan locks eyes with her target, aiming the paper right at it. She throws. The wrapper flies in the air and lands perfectly in the can. 
    
    NICK: YOOOOOOOO
    
    NOUR: That was pretty impressive
    
    NICK: Ok let me throw out the rest
    
    Ms Suwan lets out a slight grin, feeling very accomplished with her victory.
    
    Nick throws each piece into the trash until there are no more pieces left
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
