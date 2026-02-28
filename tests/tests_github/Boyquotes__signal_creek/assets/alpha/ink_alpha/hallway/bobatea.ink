VAR drinkCount = 0

== bobatea ==

    {
        - currentWorld == "dream" && bobatea_real:
            -> bobatea_realvisited
            
        - currentWorld == "dream":
            -> bobatea_dream
            
        - currentWorld == "real" && bobatea_dream:
            -> bobatea_dreamvisited
        
        - else:
            -> bobatea_real
    }

//---------------- DREAM WORLD ----------------

== bobatea_dream ==

    The vending machine is full of cool Japanese drinks, like Ramune and Boba Tea. There are other things in there, marketed by words that Nick can't understand, but their bright and bold letters and mascots make him smile.
    
    He runs over to the machine, peering in through the glass case.
    
    :NICK:
    
    * [Inspect the vending machine closer.]
        -> inspect_vendingmachine_firsttime
        
    * [Walk away.] Though it interests him greatly, he looks over at Nour and Ms.Suwan, embarassed, and steps away from the machine before he can really appreciate the drinks.
        -> END


===inspect_vendingmachine_firsttime===

    Upon further inspection, it seems the vending machine has been recently stocked. Not a single drink taken out yet. The panel on the side blinks enticingly - "BUY ONE FOR A DOLLAR, GET ANOTHER ONE FREE."
    
    NICK: Look! These look so tasty! Maybe we can take a break to have some? If we buy...
    
    He takes a moment to count on his fingers before exclaiming.
    
    Three! If we buy three we can each get two, and it'll only cost us three bucks! What a steal!
    
    :THE PARTY:
    
    *NOUR: Oh... I think I'll pass.
        -> nour_doesnt_like_boba
        
    *MS SUWAN: Oh! I didn't realize they sold Boba in vending machines. Perhaps I'll try one, later.
        -> nick_ill_hold_you_to_it


===nour_doesnt_like_boba===

    NICK: You don't like Boba, Nour???
    
    NOUR: Ah... not really, no. I prefer coffee.
    
    NICK: Gross. Well... maybe we'll find a coffee dispenser for you somewhere. Or maybe one of those fancy drinks from SandDollars. You think there's a SandDollars around?
    
    NOUR: SandDollars is a horrible company. If I had to choose, I'd take no coffee instead of their coffee.
    
    NICK: Okayyy... No SandDollars... got it.
    
    -> END


===nick_ill_hold_you_to_it===

    NICK: Well then I'll hold you to it later!
    
    MS SUWAN: Of course. Maybe once we explore a little more we can have a Boba break.
    
    NICK: Hell yeah... Boba Break Time! Let's get going, then. 
    
    -> END


== bobatea_realvisited ==

    {drinkCount < 2:
    
        The vending machine is once again full of cool Japanese drinks, like Ramune and Boba tea and other things that Nick can't understand.
        
        He looks wistful as he approaches.
        
        NICK: Thank god it's not coffee. That shit is rancid. This is much better.
        
        Nour makes a face, visibly disagreeing with the sentiment, but says nothing.
        
        NICK: So... what do you say, Ms.Suwan? Want to buy that drink now?
        
        Ms.Suwan seems to think for a moment, before smiling briefly, mostly with her eyes. 
        MS SUWAN: It's been quite an eventful day so far. Perhaps a drink is warranted. It's buy one, get one free, yes? I could buy one and give you the other-
        
        NICK: NO! I mean... sorry. No it's alright, we can split the cost so it's just 50 cents each, yeah?
        
        Ms.Suwan seems taken aback for a moment, but after a second she nods slowly. 
        
        MS SUWAN: Yes, of course. That sounds reasonable. Do you want to insert the coins or should I?
        
        NICK: Here! Let me do it.
        
        He takes the coins from Ms.Suwan, fishing out 50 cents of his own, and puts them into the machine. It seems the machine knows what both he and Ms.Suwan knows without need for any input, as two bottles slide out, one strawberry, one melon.
        
        Nick takes his happily, giving the melon one to Ms.Suwan. 
        
        ~ drinkCount = drinkCount + 1
        
    }

    {drinkCount > 2:
    
        The vending machine is once again full of cool Japanese drinks, like Ramune and Boba tea and other things that Nick can't understand.
        
        He looks wistful as he approaches. He doesn't seem eager to buy another drink.
        
        MS SUWAN: Are you alright, Nick? 
        
        NICK: Hm? Oh... yeah. Do you think since it's the dream world... I can buy an infinite amount of Boba? I don't have any more coins.
        
        MS SUWAN: ... Maybe? Perhaps you can try? I'm not sure what you'd try but... you can will it into existence?
        
        Nick seems to focus all his attention on the machine, squinting very hard. After a few moments, it makes a noise, causing Nick to look in excitement... but nothing happens, and he looks disappointed.
        
        MS SUWAN: Do you want me to buy you a drink this time? It's still buy one, get one free.
        
        NICK: It's alright, thank you Ms.Suwan. I've had enough tea already, honestly. This is probably better before I get diabetes, lol. 
    
    }

    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== bobatea_real ==

    The vending machine is full of coffee, sparkling water, and diet sodas. Nick's nose scrunches up.
    
    NICK: I swear they used to have better stuff in here last I was here. I'm not buying any of this crap, it's not worth it.
    
    Nour looks at the assortment of drinks and seems equally unimpressed. 
    
    NOUR: They don't even have the good brands. These are more like liquid sugar than coffee... and the bad kind of liquid sugar, too.
    
    MS SUWAN: No point dwelling on the vending machine, then. Maybe there'll be better drinks around somewhere else.
    
    -> END


== bobatea_dreamvisited ==

    The vending machine is full of coffee, sparkling water, and diet sodas again. Nick's nose scrunches up.
    
    NICK: The backrooms knows whats up. What the hell is all this boring shit? Boba and Ramune are so much better. We shoud go back, I'm thirsty... and not for any of this.
    
    -> END

//SUBSEQUENT REAL KNOTS HERE


