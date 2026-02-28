=== meeting_demonstrate_interactive

    *   [DRAFT 1] -> draft1 -> 
    *   [DRAFT 2] -> draft2 -> 
    *   [DRAFT 3] -> final_draft -> 
    
    -   ->->
    
= draft1 

-   (demonstrate_opts) 

    Tyrell:     I want to see it work... 

*   Deckard:    Where's the subject? 

*   Deckard:    What's that going to prove?
        
*   Deckard:    On you?

-   { stopping:
    -   -> work_on_a_person -> 
    
    -   -> indulge_me -> 

    -   Tyrell:     Try her. 
        ->->
    }
-   (loop)  -> demonstrate_opts 


- (work_on_a_person)
    Tyrell:     I want to see it work on a person. 
    Tyrell:     I want to see a negative before I provide you with a positive. 
    ->->
    
- (indulge_me)
    Tyrell:     Indulge me. 
    ->-> 

= draft2

// apply conditions to Deckard's lines

    Tyrell:     I want to see it work. 

-   (demonstrate_opts) 

*   Deckard:    Where's the subject? 

*   {came_from(->work_on_a_person)} 
    Deckard:    What's that going to prove?
        
*   {work_on_a_person}
    Deckard:     On you?

-   { stopping:
    -   -> work_on_a_person -> 
    
    -   -> indulge_me -> 

    -   Tyrell:     Try her. 
        ->->
    }
-   (loop)  -> demonstrate_opts 

- (work_on_a_person)
    Tyrell:     I want to see it work on a person. 
    Tyrell:     I want to see a negative before I provide you with a positive. 
    ->->
    
- (indulge_me)
    Tyrell:     Indulge me. 
    ->-> 
        
    
= final_draft 

- (demonstrate)
    Tyrell:     I want to see it work... 

// add a deflect that loops
// add a trapdoor that isn't a trapdoor 
// add a deflect that doesn't deflect

-   (demonstrate_opts) 

// A deflection; this one keep Tyrell quiet for a turn
*   {came_from(-> demonstrate)} 
    Deckard:    I know you're not a replicant, Mr Tyrell.
    Tyrell:     You do, do you?
    -> demonstrate_opts

// A deflection; but makes more sense after the conversation has gone on a bit
*   {not came_from(-> demonstrate)} 
    Deckard:    Mr Tyrell, I'm here on business. 
    { indulge_me:
        Tyrell:     My work here is your business.
    }
    
*   {not work_on_a_person || indulge_me} 
    Deckard:    Where's the subject? 

// A deflection; Tyrell ignores it
*   Deckard:    You must have seen it before. 
    { work_on_a_person:
        Tyrell:     Never in the hands of an expert.
    }

*   { came_from(-> work_on_a_person) }
    Deckard:    What's that going to prove?
        
*   {work_on_a_person}  
    Deckard:    On you?
    
-   { stopping:
    -   -> work_on_a_person -> 
    
    -   -> indulge_me -> 

    -   -> try_her

    }
-   (loop)  -> demonstrate_opts 

- (work_on_a_person)
    Tyrell:     I want to see it work on a person. 
    Tyrell:     I want to see a negative before I provide you with a positive. 
    ->->
    
- (indulge_me)
    Tyrell:     Indulge me. 
    ->-> 
    
- (try_her) 
    Tyrell:     Try her. 
    ->->    