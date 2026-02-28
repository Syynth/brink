=== final_interactive_script
    
    # FREEFLOW
-   (do_you_like) Rachel:     Do you like our owl? 

- (opts1)

*   (dont_like_owl) {came_from(-> do_you_like)}     
    Deckard:    No.
    Rachel:     Is that so? 
    -> it_seems_you_think

*   (wecould) {came_from(-> expensive)}   
    Deckard:    Why make it?
    Rachel:     Because we could. 

*   {came_from(-> artifical) || came_from(-> expensive)}  
    Deckard:    Impressive.
    
*   (artifical) { not expensive }
    Deckard:    It's artificial? 
    Rachel:     Of course it is. 

*   (expensive) Deckard:    Must be expensive. 
    Rachel:     Very. 

*   (impatient) {opts1 > 1}  
    Deckard:    I don't like fakes.
    { artifical:
        Rachel:     Is that why you do what you do to our creations? 
        -> it_seems_you_think_opts
    - else:
        Rachel:     There are no real owls.
    }
    -> im_rachel
    
-   {once: 
    -   -> opts1 
    }
  
-   (im_rachel) Rachel:     I'm Rachel. 

*   (deckard) Deckard:    Deckard. 

*   Deckard:    I'm here to see Tyrell.
    Rachel:     He's a very busy man.
    -> may_i_ask
        
*   (tyrells_niece)     Deckard:    What do you do here, Rachel?
    Rachel:     I work for my uncle: Dr Tyrell.
    


-   (it_seems_you_think)
    Rachel:     It seems you think {tyrells_niece:what we do|our work} is not of benefit to the public. 

- (it_seems_you_think_opts)

*   Deckard:    Replicants are like any other machine...
    Deckard:    They're either a benefit or a hazard. 
    Deckard:    If they're a benefit, they're not my problem. 
    

*   Deckard:    {wecould:You could make them <i>better.</i>|Replicants are dangerous.}
    Rachel:     Our replicants are perfect.
    - - (tyrellsays) Rachel:     More human than human, that's what Dr Tyrell says.
    -> it_seems_you_think_opts
    
*   (youthink) Deckard:    {came_from(->tyrellsays):And what | What} do you {tyrellsays:say|think}<>? 
    { tyrells_niece:
        Rachel:     I agree with my uncle, of course.
    }
    
    { dont_like_owl:
        Rachel:     And I admire the owl.
    - else: 
        Rachel:     I think what we do here is vital to humanity.
    }
    

*   {tyrellsays} 
    Deckard:    {impatient:Is Tyrell coming? |I want to talk to Tyrell.}
    Rachel:     He knows you're here{deckard:<>, Mr Deckard<>}.

    
-  (may_i_ask)
// let's do the same for the second part of it

    Rachel:     May I ask you a personal question? 
      
- (opts3) 
*   Deckard:    {opts3 > 1:Go ahead|Sure}.

*   (owl) Deckard:    {impatient || dont_like_owl:Is it about the owl?|What about?}
    Rachel:   {impatient || dont_like_owl:It's about| About} {impatient || dont_like_owl:you. About} your job. 
    -> opts3 
    
*   Deckard:    I'm not here to answer your questions. 
    { tyrells_niece || (youthink && tyrellsays) :
        Deckard:    Whoever's niece you are.
    }
    -> tyrell_enters

*   {came_from(-> owl)}  Deckard:    My job isn't personal.
    Rachel:     It is for the replicants you retire.
    -> tyrell_enters
    
-   Rachel:     Have you ever retired a human by mistake? 

- (opts4)

*   (nope) Deckard:    No.

*   (notsure)   Deckard:    I don't know for sure.
    Deckard:    I don't think so. 
    
*   (likethat) Deckard:    You'd like that, wouldn't you?
    Deckard:    Show how good your product is? 
    Rachel:     But have you?
    -> opts4
    
*   {came_from(-> likethat)} 
    Deckard:    If you cared, you would put labels on them. 
    
-   Rachel:     {nope:But|So} in your position that is a risk?  
 
- (tyrell_enters)  

    Tyrell:     Is this to be an empathy test? 
    Tyrell:     Capillary dilation? The so-called blush response? 
    Tyrell:     Fluctuation of the pupil?
    Tyrell:     Involuntary dilation of the iris. 
    
*   Deckard:    We call it Voigt-Kampff for short.
    
*   Deckard:    You're Tyrell? 
    Tyrell:     Dr Eldon Tyrell. 
    Tyrell:     You've brought your machine? 
   -> demonstrate

*   (noempathy) Deckard:    Replicants have no empathy to test.
    Deckard:    You didn't give them any.
    Deckard:    They're like owls. 
    
-   {deckard: Rachel:     Mr Deckard, Dr Eldon Tyrell. }
    
    Tyrell:     Demonstrate it.
    
-   (demonstrate) 
    Tyrell:     I want to see it work.

-   (demonstrate_opts) 

*   (yourenot) {came_from(-> demonstrate)} 
    Deckard:    I know you're not a replicant, Mr Tyrell.
    Tyrell:     You do, do you? How perceptive you are.
    -> demonstrate_opts

*   {came_from(-> yourenot)}  
    Deckard:    You don't need the machine to see empathy.
    Deckard:    You need it to see when it's missing.

*   (business) {not came_from(-> demonstrate)}  {not indulge_me }
    Deckard:    Mr Tyrell, I'm here on business. 
    { indulge_me:
        Tyrell:     My work here is your business.
    }

*   {not work_on_a_person || indulge_me} {not came_from(-> yourenot)} 
    Deckard:    Where's the subject? 

*   Deckard:    You must have seen it before. 
    { work_on_a_person:
        Tyrell:     Never in the hands of an expert.
    }

*   { came_from(-> work_on_a_person) }
    Deckard:    What's that going to prove?
        
*   {work_on_a_person}  {CHOICE_COUNT() < 3}
    Deckard:    On you?

    
-   { stopping:
    -   -> work_on_a_person -> 
    
    -   -> indulge_me -> 

    -   -> try_her
    }
-   -> demonstrate_opts 

- (work_on_a_person)
    Tyrell:     I want to see it work on a person. 
    Tyrell:     I want to see a negative before I provide you with a positive. 
    ->->
    
- (indulge_me)
    Tyrell:     Indulge me. 
    ->->  
    
= try_her 
    Tyrell:     Try {came_from(->business):your machine on} her. 
    ->->