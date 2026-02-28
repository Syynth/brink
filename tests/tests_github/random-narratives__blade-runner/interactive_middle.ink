=== meeting_rachel_rachel_to_personalQuestion_interactive

    <i>Blah blah owls blah blah</i>

    *   [DRAFT 1]   -> draft1 -> 
    *   [DRAFT 2]   -> draft2 ->
    *   [DRAFT 3]   -> draft3 ->
    *   [DRAFT 4]   -> draft4 ->
    *   [DRAFT 5]   -> final_draft ->
    
    -   ->->
    

= draft1

// simple accelerate trapdoor

-   Rachel:     I'm Rachel. 

*   Deckard:    Deckard. 

*   Deckard:    I'm here to see Tyrell.
    Rachel:     He's a very busy man.
    ->->

-   (it_seems_you_think)
    Rachel:     It seems you think our work is not of benefit to the public. 

*   Deckard:    Replicants are like any other machine...
    Deckard:    They're either a benefit or a hazard. 
    Deckard:    If they're a benefit, they're not my problem. 
    
-  ->-> 
    

= draft2

// opportunity for the player to actually ask a question
// we haven't actually let the player do that at all yet!

-   Rachel:     I'm Rachel. 

*   Deckard:    Deckard. 

*   Deckard:    I'm here to see Tyrell.
    Rachel:     He's a very busy man.
    ->->
        
*   Deckard:    What do you do here, Rachel?
    Rachel:     I'm Tyrell's niece. 

-   (it_seems_you_think)
    Rachel:     It seems you think our work is not of benefit to the public. 

*   Deckard:    Replicants are like any other machine...
    Deckard:    They're either a benefit or a hazard. 
    Deckard:    If they're a benefit, they're not my problem. 
    

-    ->-> 

= draft3

// let's do something with that second bit, which is one of the worst lines in the whole original film
//  So let's add some 

-   Rachel:     I'm Rachel. 

*   Deckard:    Deckard. 

*   Deckard:    I'm here to see Tyrell.
    Rachel:     He's on his way up. 
    ->->
        
*   Deckard:    What do you do here, Rachel?
    Rachel:     I'm Tyrell's niece. 

-   (it_seems_you_think)
    Rachel:     It seems you think our work is not of benefit to the public. 

- (opts)

*   Deckard:    Replicants are like any other machine...
    Deckard:    They're either a benefit or a hazard. 
    Deckard:    If they're a benefit, they're not my problem. 
    
*   Deckard:    You could make them better. 
    Rachel:     Our replicants are perfect.
    Rachel:     More human than human, that's what Dr Tyrell says.
    
*   Deckard:    What do you think? 
    Rachel:     It doesn't matter what I think.


-    ->-> 

= draft4

//  better with a loop on the reject option, so Rachel pushes back

-   Rachel:     I'm Rachel. 

*   Deckard:    Deckard. 

*   Deckard:    I'm here to see Tyrell.
    Rachel:     He's on his way up. 
    ->->
        
*   Deckard:    What do you do here, Rachel?
    Rachel:     I'm Tyrell's niece. 

-   (it_seems_you_think)
    Rachel:     It seems you think our work is not of benefit to the public. 

- (opts)

*   Deckard:    Replicants are like any other machine...
    Deckard:    They're either a benefit or a hazard. 
    Deckard:    If they're a benefit, they're not my problem. 
    
*   Deckard:    You could make them better. 
    Rachel:     Our replicants are perfect.
    Rachel:     More human than human, that's what Dr Tyrell says.
    -> opts

*   Deckard:    What do you think? 
    Rachel:     It doesn't matter what I think.


-    ->->  
  
        
= final_draft

//  Tidy up: 3 option limit; tweak text to flow better; add a final trapdroo

-   Rachel:     I'm Rachel. 

*   Deckard:    Deckard. 

*   Deckard:    I'm here to see Tyrell.
    Rachel:     He's a very busy man.
    ->->
        
*   Deckard:    What do you do here, Rachel?
    Rachel:     I'm Tyrell's niece. 


-   (it_seems_you_think)
    Rachel:     It seems you think our work is not of benefit to the public. 

- (opts)

*   Deckard:    Replicants are like any other machine...
    Deckard:    They're either a benefit or a hazard. 
    Deckard:    If they're a benefit, they're not my problem. 
    

*   Deckard:    You could make them better. 
    Rachel:     Our replicants are perfect.
    - - (tyrellsays) Rachel:     More human than human, that's what Dr Tyrell says.
    -> opts
    

*   Deckard:    {came_from(->tyrellsays):And what | What} do you think? 
    Rachel:     It doesn't matter what I think.    

*   {tyrellsays} Deckard:    Is Tyrell coming? 
    Rachel:     He knows you're here, Mr Deckard.
    
-    ->-> 

