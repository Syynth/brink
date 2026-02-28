INCLUDE linear_film_version.ink
INCLUDE basic_game_version.ink
INCLUDE interactive_opening.ink
INCLUDE interactive_middle.ink
INCLUDE interactive_final.ink
INCLUDE interactive_tyrell.ink
INCLUDE interactive_full_script.ink
INCLUDE interactive_final_script.ink
INCLUDE interactive_demonstrate.ink



# theme: dark




// https://www.youtube.com/watch?v=ndnd-ERnWew
-> mainPicker


=== mainPicker
# CLEAR
+   [Film Version]
    -> meeting_rachel_linear -> 
    
+   [Game Version]
    -> meeting_rachel_game -> 
    
+   (pieces) [Interactive Fiction Version: Piece by piece]
    
    + +     [1. The Owl]
            -> meeting_rachel_owl_to_rachel_interactive ->
            
            {skip_Rachels_name: 
                Rachel:     It seems you think our work is not of benefit to the public...
            - else:
                Rachel:     I'm Rachel...
            }
    
    + +     [2. Our Work is Not Of Benefit...]
            -> meeting_rachel_rachel_to_personalQuestion_interactive ->  
            Rachel:     May I ask you a personal question?
    
    + +     [3. A Personal Question]
            -> meeting_rachel_personalQuestion_to_tyrell_interactive 
            
            Tyrell:     Is this to be an empathy test..?
    
    + +     [4. Tyrell Enters]
            -> meeting_rachel_tyrell_interactive ->
            
    + +     [5. Tyrell Wants Demonstration] 
            -> meeting_demonstrate_interactive ->
    
    + +     [Back]  -> mainPicker 
    
    - -     <i>...scene continues...</i> 
    + +     [Continue...] 
            # CLEAR
            -> pieces

+   [Interactive Fiction Version: Full] 
    -> meeting_rachel_interactive_full_script -> 

-

+     [Continue...] 

-   -> mainPicker 

 === function came_from(-> x)  
    ~ return TURNS_SINCE(x) == 0

