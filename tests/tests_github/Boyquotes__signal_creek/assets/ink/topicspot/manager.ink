== manager ==

    {
        - !manager_firstconvo:
            -> manager_firstconvo
    }
    
    MANAGER: "Let me know if you need help finding anything!"
    
    &PORTRAIT_manager_manageruwu
    
    MANAGER: "Or not... I'm kinda too lazy to get up. O3o"
    
    &PORTRAIT_manager_managerneutral
    
    NOUR: "Actually..."
    
    -> manager_wayout


== manager_wayout ==
    
    * "Do you know a way to get out of here?"
    
        NOUR: "I think the elevator is stuck. Is there another way off of this floor?"
        
        &PORTRAIT_manager_manageruwu
        
        MANAGER: "Someone should be fixing that soon. You could take the escalator instead~!"
        
        &PORTRAIT_manager_managerneutral
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "I don't think that's working either."
        
        &PORTRAIT_nour_nourneutral
        
        MANAGER: "Well, then you can hang out here with me while you wait!"
        
        -> manager_wayout
    
    * "Why does everything look so strange?"
    
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "Things are all topsy-turvy."
        
        NOUR: "People are appearing and disappearing, the elevator's acting all weird..."
        
        NOUR: "And the trees don't look right."
        
        NOUR: "<i>You</i> don't look right."
        
        &PORTRAIT_nour_nourneutral
        
        &PORTRAIT_manager_managerunu
        
        MANAGER: "Hey! ;-; That isn't very nice."
        
        &PORTRAIT_manager_managerneutral
        
        MANAGER: "Everything looks fine to me. Maybe you should get your eyes checked."
        
        -> manager_wayout
         
    + -> 
    
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "Y'know what, never mind. Seems like you can't help us get out of here."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: "Thanks anyways."
    
        -> END


== manager_firstconvo ==
    
    &PORTRAIT_manager_manageruwu
    
    MANAGER: "Welcome to TopicSpot! UWU"
    
    &PORTRAIT_manager_managerneutral
    
    MANAGER: "You're in luck, we have an epic sale going on right now!"
    
    MANAGER: "Everything labelled as *-1??/.. is 90% off!"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Uhhh... Labelled as <i>what</i>, now?"
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Also, did, uh... Did you just say <i>oowoo</i> out loud?"
    
    &PORTRAIT_manager_manageruwu
    
    MANAGER: "No, I did not! UWU"
    
    &PORTRAIT_manager_managerneutral
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Yeah you did, you just did it again! How the heck?"
    
    &PORTRAIT_nick_nickneutral
    
    MS. SUWAN: "I don't think this conversation is a productive use of our time."
    
    * "Why are you saying those... symbols?"
    
        NOUR: "You said... asterisk, hyphen, one, question mark, question mark, slash."
        
        &PORTRAIT_nour_nournervous
         
        NOUR: "But with the actual symbols, not their names. How are we even understanding this right now?"
        
        MANAGER: "That's just how I learned to talk! I'm special like that owo"
        
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "Sure... alright. Forget I asked."
    
    * "I think we should go."
    
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "Forget it, Nick. Let's just pretend he's normal, we've gotta move on."
        
        &PORTRAIT_nick_nickangry
        
        NICK: "Sheesh, fine."
    
    - MANAGER: "Well, let me know if you need help finding anything. XD"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Whoa, you just did it again... seriously, how are you doing that?"
    
    MANAGER: "I don't know what you're talking about!"
    
    -> END


// == managershirtconvo ==

//     NICK: Well -- a-anyway... um... I would like to buy this shirt.
    
//     MANAGER: Oh! That shirt! Omgg xD you have good taste, kid ~ that's one of our cutest shirts! x3 Okay, let me get this rung up for you!
    
//     The manager turns around and scans the shirt's label. 
    
//     MANAGER: That'll be 39.99, please!
    
//     NICK: Huh? F-Forty dollars...? Wait, but the price tag said... huh? The price tag's blank...?!
    
//     MANAGER: It's not 40 dollars, Nick. It's 39.99. There's a difference xD Do you want a bag for it?
    
//     NICK: Wait, hold on... hold on... I don't know if I can pay forty -- sorry, thirty-nine-ninety-nine -- I don't have that kind of money on me right now...
    
//     Nick's cheeks flush red as he looks down at his barren wallet. Only a few ones and a couple of coins fill the nearly-empty space, the remains of the money he had earned through mowing his neighbor Mr. Pierre's lawn last weekend. Suddenly he wishes the earth would open up and swallow him whole.
    
//     NICK: I guess you don't do discounts, huh...
    
//     MANAGER: Yeah... I'm sorry, kid. Hey, don't feel too sad. I can hold onto this for you, if you'd like, and it'll be yours whenever you're able. x3 
    
//     NICK: No, no... it's fine. I shouldn't be spending forty-- I mean, thirty-nine-ninety-whatever on stuff like this anyway. 
    
//     Feeling frustrated and guilty, Nick turns around and walks away. The world around him begins to change.
    
//     &NICK: Ugh...
    
//     -> END

