=== meeting_rachel_personalQuestion_to_tyrell_interactive 

    *   [DRAFT 1] -> draft1 -> 
    *   [DRAFT 2] -> draft2 -> 
    *   [DRAFT 3] -> final_draft -> 
    
    -   ->->
    

= draft1 

// Provide a standard three way branch: accept, deflect, or skip
// deflection repeats the hub, we bring in a new unique response

    Rachel:     May I ask you a personal question? 
    
- (opts) 
*   Deckard:    Sure.

*   (owl) Deckard:    Is it about the owl?
    Rachel:     It's about you. About your job. 
    -> opts 
    
*   Deckard:    I'm not here to answer your questions. 
    ->->

-   Rachel:     Have you ever retired a human by mistake? 

*   Deckard:    No.

-   Rachel:     But in your position that is a risk?  

    ->->
    
= draft2

// Tidy, build out choices

    Rachel:     May I ask you a personal question? 
    
- (opts) 
*   Deckard:    {opts > 1:Go ahead|Sure}.

*   (owl) Deckard:    Is it about the owl?
    Rachel:     It's about you. About your job. 
    -> opts 
    
*   Deckard:    I'm not here to answer your questions. 
    ->->

*   {came_from(-> owl)}  Deckard:    My job isn't personal.
    Rachel:     It is for the replicants you retire.
    ->->

-   Rachel:     Have you ever retired a human by mistake? 

*   Deckard:    No.

-   Rachel:     But in your position that is a risk?  

    ->->    
    
= final_draft 

// let's do the same for the second part of it

    Rachel:     May I ask you a personal question? 
      
- (opts) 
*   Deckard:    {opts > 1:Go ahead|Sure}.

*   (owl) Deckard:    Is it about the owl?
    Rachel:     It's about you. About your job. 
    -> opts 
    
*   Deckard:    I'm not here to answer your questions. 
    ->->

*   {came_from(-> owl)}  Deckard:    My job isn't personal.
    Rachel:     It is for the replicants you retire.
    ->->
    
-   Rachel:     Have you ever retired a human by mistake? 

- (opts_retiring)

*   (nope) Deckard:    No.

*   (notsure)   Deckard:    I don't know for sure.
    Deckard:    I don't think so. 
    
*   (likethat) Deckard:    You'd like that, wouldn't you?
    Deckard:    Show how good your product is? 
    Rachel:     But have you?
    -> opts_retiring
    
*   {came_from(-> likethat)} 
    Deckard:    If you cared, you would put labels on them. 
    
-   Rachel:     {nope:But|So} in your position that is a risk?  

    ->->