== rina ==

    {
        - currentWorld == "dream" && rina_real:
            -> rina_realvisited
            
        - currentWorld == "dream":
            -> rina_dream
            
        - currentWorld == "real" && rina_dream:
            -> rina_dreamvisited
        
        - else:
            -> rina_real
    }
//---------------- DREAM WORLD ----------------

// rina will be divided up differently than most other objects, into 'interactions' instead of dreamvisited/realvisited (first interaction, second, third, etc.)

// and then we add as many ===interaction=== parts as we need for the future based on other objects that the player interacts with! 

== rina_dream ==

    NOUR: "Uh... Rina?"
    
    RINA: "Yousra? It's so good to see you, oh my god!"
    
    Rina twirls her hair and smirks.
    
    RINA: "How *are* you? It's been so long!  I haven't seen you since... graduation?"
    
    NOUR: "I'm fine, I guess. And I actually go by Nour now."
    
    RINA: "Oh, that's cool..."
    
    RINA: "Anyways, how's college?"
    
    NOUR: "Good. I like my classes."
    
    RINA: "Yeah, I'm *loving* my business classes at Rutgers. You know, I'm glad I picked something stable to major in."
    
    Nour shuffles, nervously.
    
    RINA: "You look, uh... different from back in highschool."
    
    NOUR: "Yeah. I know. So what?"
    
    RINA: "*Oh*, alright. I see how it is."
    
    RINA: "You know, something seems off about you."
    
    Nour clenches her jaw and exhales sharply.

:NOUR:

    + [Say nothing.] Nour says nothing, as she always did.
        -> saynothing

=saynothing

    RINA: "Oh, don't look like that, Yousra! I'm just joking. You know we're still best friends, right? We've been like *sisters* since we were little kids!"
    
    RINA: "Remember when we came to the mall in sixth grade, and got best friend bracelets from Claira's?"
    
    NOUR: "You couldn't even stand up for your "best friend" when our other friends were having that argument at the lunch table."
    
    NOUR: "They said people like me shouldn't be in public, that gay rights were going to morally corrupt our society."
    
    NOUR: "And you were fucking *silent*."
    
    RINA: "I didn't know what to *do*! I didn't want them to hate me-"
    
    NOUR: "And then, when we got into a fight, you threw your bracelet at me, and called me a freak."
    
    RINA: "You know I was just kidding. It was because you were overreacting!"
    
    RINA: "But seriously, I'm really, really sorry."
    
    :NOUR:
    
        * [Stand down.] Nour decides to stand down, remaining silent.
            -> silent
            
        * ["Bullshit, I know you're not really sorry."] NOUR: "Bullshit, I know you're not really sorry. You're a fucking liar. Overreacting??"
            -> bullshit
            
        * [Right, you're sorry. Fine.] NOUR: "Right, you're sorry. Fine. But it was obvious which side you're really on."
            -> whichsideyoureon
            
        * -> nothingelse
    
    -> DONE

//Nour remains complacent
=silent
    
    RINA: "Hey, you heard me, right? I'm really sorry."
    
    RINA: "I didn't mean to call you a freak, I was just kidding!"
    
    NOUR: "Yeah, right..."
    
    &NOUR: "That's what everyone says."
    
    -> DONE

//in this path, nour escalates the situation super quickly, and it blows up at her.
=bullshit

    NOUR: "I have feelings, too. If you really cared about me, you would've stood up for me in that fight."
    
    NOUR: "But *no*, you're too self-centered. You fucked up, and hurt me just as much as they did. No, *more* than they did."

    RINA: "You *bitch*! I was the only person in our friend group who even talked to you after that conversation, everyone else thought you were gross."
    
    Rina is on the verge of tears, now.
    
    RINA: "You should be grateful."
    
    NOUR: "Grateful for *what*? You said NOTHING."
    
    NOUR: "I blame you for *so* much of my suffering in high school. I didn't have any friends until college. I wish you were sorry for what you did. You shallow, selfish-"
    
    &As she starts sobbing, Rina disappears from view.
    
    -> DONE


//in this path, nour is on the verge of escalating things
=whichsideyoureon

    RINA: "God, Yousra, I don't know how many times I can say I'm sorry. What do you want me to tell you?"
    
    Nour clenches her fist.
    
    NOUR: "I mean..."
    
    :NOUR:
    
    ** ["Why the hell are you STILL pretending to be my friend?"]NOUR: "Why the hell are you STILL pretending to be my friend? I know you agreed with them behind my back.
        -> behindmyback
        
    ** ["Whatever, I guess."]NOUR: "Whatever, I guess."
        -> whatever


//things blow up after a steady escalation   
=behindmyback

    RINA: "I, uh, yeah..."
    
    Rina glances sideways, lip trembling.
    
    NOUR: "I just, I just... I thought we were supposed to be best friends. I thought I could TRUST you."
    
    NOUR: "Nobody talked to me after that. And you're just as bad as they are, because you said *nothing*."
    
    Nour raises her fist.
    
    NOUR: "You know, that really fucked me up. That really, really fucked me up."
    
    RINA: "I just didn't want them to hate me, too!"
    
    Rina starts tearing up, her voice shaking.
    
    RINA: "I didn't want them to think I was, like, you know..."
    
    &NOUR: "A freak? Gay? like me?"
    
    -> DONE

=whatever

    RINA: "That's it? You're going to say all that, and then drop it?"
    
    NOUR: "Whatever. You'll never care about me. I can't make you be sorry."
    
    &NOUR: "I wish I could make you be sorry."
    
    -> DONE

=nothingelse

    Nour has nothing else to say to Rina. What's done is done.
    
    Rina stares at her, blankly.
    
    &Nour's fists loosen, and Rina fades from view.
    
    The party comforts Nour, attempting to help her move past this incident.
    
    -> END
 

== rina_realvisited ==

    +   -> rina_dream

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== rina_real ==

    +   -> rina_dream

== rina_dreamvisited ==

    +   -> rina_dream

//She's not in the real world, if these redirects are needed, there's an error
