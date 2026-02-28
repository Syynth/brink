
=== meeting_rachel_owl_to_rachel_interactive

*   [DRAFT 1]   -> draft1 ->
*   [DRAFT 2]   -> draft2 ->
*   [DRAFT 3]   -> draft3 ->
*   [DRAFT 4]   -> draft4 ->
*   [DRAFT 5]   -> final_draft ->

-   ->->

= draft1
    
// the first two beats of the conversation are mostly a pacing element; they don't convey anything important
// so let's make that into a choice, and a short looped hub
    
-   Rachel:     Do you like our owl?

- (opts)

*   (artifical) Deckard:    It's artificial? 
    Rachel:     Of course it is. 

*   (expensive) Deckard:    Must be expensive. 
    Rachel:     Very. 
    
-   {once: 
    -   -> opts 
    }
    ->->
    
    
= draft2 

// choosing option 2 before 1 doesn't flow very well
// so we prevent the "artifical" comment, and replace it with something else
  
-   Rachel:     Do you like our owl?

- (opts)

*   {came_from(-> expensive)}   Deckard:    Why make it?
    Rachel:     Because we could. 
    
*   (artifical) { not expensive }
    Deckard:    It's artificial? 
    Rachel:     Of course it is. 

*   (expensive) Deckard:    Must be expensive. 
    Rachel:     Very. 

-   {once: 
    -   -> opts 
    }

-   ->->

= draft3 

// run out of choices, so let's add something in
  
-   Rachel:     Do you like our owl?

- (opts)

*   {came_from(-> expensive)}   Deckard:    Why make it?
    Rachel:     Because we could. 

*   {came_from(-> artifical) || came_from(-> expensive)}  
    Deckard:    Impressive.
    
*   (artifical) { not expensive }
    Deckard:    It's artificial? 
    Rachel:     Of course it is. 

*   (expensive) Deckard:    Must be expensive. 
    Rachel:     Very. 

-   {once: 
    -   -> opts 
    }
    
-   ->->


= draft4 

// reject the owl conversation, to give the player the sense they can push forwards
  
-   (do_you_like) Rachel:     Do you like our owl?

- (opts)

*   {came_from(-> expensive)}   Deckard:    Why make it?
    Rachel:     Because we could. 

*   {came_from(-> artifical) || came_from(-> expensive)}  
    Deckard:    Impressive.
    
*   (artifical) { not expensive }
    Deckard:    It's artificial? 
    Rachel:     Of course it is. 

*   (expensive) Deckard:    Must be expensive. 
    Rachel:     Very. 

*   {opts > 1}  Deckard:    I'm not here about your owl.
    
-   {once: 
    -   -> opts 
    }

-   ->->


= final_draft

// finally, a "press forward" trap door for people who really want to be blunt
// These are great! They accelerate the conversation; you don't need to write more 
// content, but they feel really responsive

VAR skip_Rachels_name = false
  
-   (do_you_like) Rachel:     Do you like our owl?

- (opts)

*   {came_from(-> do_you_like)} Deckard:    No.
    ~ skip_Rachels_name = true
    ->-> 

*   {came_from(-> expensive)}   Deckard:    Why make it?
    Rachel:     Because we could. 

*   {came_from(-> artifical) || came_from(-> expensive)}  
    Deckard:    Impressive.
    
*   (artifical) { not expensive }
    Deckard:    It's artificial? 
    Rachel:     Of course it is. 

*   (expensive) Deckard:    Must be expensive. 
    Rachel:     Very. 

*   {opts > 1}  Deckard:    I'm not here about your owl.
    
-   {once: 
    -   -> opts 
    }
    
-   ->->


    