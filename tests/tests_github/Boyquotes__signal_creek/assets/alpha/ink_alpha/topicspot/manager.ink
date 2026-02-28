== manager ==

    {
        - currentWorld == "dream" && manager_real:
            -> manager_realvisited
            
        - currentWorld == "dream":
            -> manager_dream
            
        - currentWorld == "real" && manager_dream:
            -> manager_dreamvisited
        
        - else:
            -> manager_real
    }
//---------------- DREAM WORLD ----------------

== manager_dream ==

    //Dream world visited first, before real
    
    MANAGER: Hello and welcome to TopicSpot! UWU
    
    NICK: Oh, hi... you look a little different from how I remember...
    
    NICK: Also, did- did you just say UWU out loud? 
    
    MANAGER: No, I did not! UWU 
    
    NICK: Uh... okay--
    
    MANAGER: If there's anything I can do for you, please let me know ~ xD 
    
    NICK: Whoa, you just did it again... how are you doing that?
    
    MANAGER: I don't know what you're talking about!
    
    + { manager_convo } -> managershirtconvo
    
    -> END


== manager_realvisited ==
    
    //Dream world after visiting real first
    
    MANAGER: Hello and welcome to TopicSpot! UWU
    
    NICK: Wow, you look kinda different... wait, did you just say UWU out loud??
    
    MANAGER: No, I did not! UWU 
    
    NICK: Uh... okay--
    
    MANAGER: If there's anything I can do for you, please let me know ~ xD 
    
    NICK: Whoa, you just did it again... how are you doing that?
    
    MANAGER: I don't know what you're talking about!
    
    -> END

//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== manager_real ==

    MANAGER: Hello, and welcome to TopicSpot... is there anything I can do for you...
    
    He seems like he is hoping you'll say no.
    
    NICK: Nothing right now. Thanks.
    
    MANAGER: Just holler if you wanna buy something. Remember, everything's discounted for the closing sale. Please buy something.
    
    NICK: Okay...
    
    -> END


== manager_dreamvisited ==
    
    //Real world after visiting dream first
    
    MANAGER: Hello, and welcome to TopicSpot... is there anything I can do for you...
    
    NICK: Just like how I remember, again...
    
    MANAGER: What was that?
    
    NICK: Oh - nothing. Sorry.
    
    MANAGER: Listen, kid, if you're not gonna buy anything then stop wasting my time and skedaddle!
    
    NICK: Jeez-- okay, fine!
    
    MANAGER: And you can go tell your friends about the closing sale!
    
    -> END

//---------------- COOL SHIRT CONNECTION ----------------

== managershirtconvo ==

    NICK: Well -- a-anyway... um... I would like to buy this shirt.
    
    MANAGER: Oh! That shirt! Omgg xD you have good taste, kid ~ that's one of our cutest shirts! x3 Okay, let me get this rung up for you!
    
    The manager turns around and scans the shirt's label. 
    
    MANAGER: That'll be 39.99, please!
    
    NICK: Huh? F-Forty dollars...? Wait, but the price tag said... huh? The price tag's blank...?!
    
    MANAGER: It's not 40 dollars, Nick. It's 39.99. There's a difference xD Do you want a bag for it?
    
    NICK: Wait, hold on... hold on... I don't know if I can pay forty -- sorry, thirty-nine-ninety-nine -- I don't have that kind of money on me right now...
    
    Nick's cheeks flush red as he looks down at his barren wallet. Only a few ones and a couple of coins fill the nearly-empty space, the remains of the money he had earned through mowing his neighbor Mr. Pierre's lawn last weekend. Suddenly he wishes the earth would open up and swallow him whole.
    
    NICK: I guess you don't do discounts, huh...
    
    MANAGER: Yeah... I'm sorry, kid. Hey, don't feel too sad. I can hold onto this for you, if you'd like, and it'll be yours whenever you're able. x3 
    
    NICK: No, no... it's fine. I shouldn't be spending forty-- I mean, thirty-nine-ninety-whatever on stuff like this anyway. 
    
    Feeling frustrated and guilty, Nick turns around and walks away. The world around him begins to change.
    
    &NICK: Ugh...
    
    -> END

