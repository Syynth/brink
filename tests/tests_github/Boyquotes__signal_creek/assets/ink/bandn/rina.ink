//Temporary variables, until we decide what knot these routes end on

//rina_state can be start, bandn, topicspot, or hallway
//rina_state is altered from the other objects

== rina ==
    
    //if this isn't the first time talking to Rina, go straight to the choices
    {
        - rina_bandn:
            -> rina_branches
            
        -!rina_bandn && !polaroid:
            -> rina_gotopolaroid
            
        - polaroid:
            -> rina_bandn
    }
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: <i>I can't talk to her. I don't even want to see her.</i>
    
    -> END


== rina_start ==
    //Everyone is pooped out of the elevator
    //they walk down 
    //elevator door CLOSES
    
    &FOLLOW_NICK_NickOutElevatorPos
    
    &FOLLOW_NOUR_NourOutElevatorPos
    
    &FOLLOW_SUWAN_SuwanOutElevatorPos
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Ugh... I feel sick."
    
    &PAUSE_0.5
    
    &ELEVATOR_SHUT
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Woah, this is crazy! Are you seeing what I'm seeing?"
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "This... this is one of the floors of the mall, isn't it? The top floor?"
    
    &PORTRAIT_nour_noursad
    
    NOUR: "But it feels... something feels off."
    
    &EMOTE_NICK_UpIdle
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "What's with all those lights at the top of the elevator?"
    
    &EMOTE_NOUR_UpIdle
    
    &FOLLOW_SUWAN_SuwanPhonePos
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "This floor was shut down <i>months</i> ago. There shouldn't even be electricity."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "We're in a place that shouldn't exist!?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Maybe we're in some kind of liminal space!"
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "...Pardon?"
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Y’know, liminal spaces? Weird alternate dimensions where everything feels off?"
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "A place where space and time aren’t acting as they should?!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "..." 
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "We're not in a science fiction novel, young man."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "I have to call one of my higher-ups about this."
    
    Ms. Suwan checks her phone in an attempt to find a connection.
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Hmm. No signal and no WiFi."
    
    Nick pulls out his phone, then frantically taps on the screen.
    
    &EMOTE_NICK_RightIdle
    
    &PORTRAIT_nick_nicksad
    
    NICK: "Aw, man. Same! I can't play any of my phone games..."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "Is that really what you-"
    
    &MOV_RINA_START
    
    A petite girl with olive skin and dyed strawberry hair appears before the group.
    
    Her entire body is still, like a mannequin. A lifelike, breathing mannequin.

    &PORTRAIT_nick_nickshocked
    
    NICK: "<i>WHAT THE-</i>"
    
    &EMOTE_SUWAN_LeftIdle
    
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "<i>You're</i> not supposed to be here either, young lady."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Hey, did you miss the part where she <i>mysteriously</i> appeared in front of us!?"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Enough with your fooling around."
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i>...Rina?!</i>
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Hey, uh... Nour? You know her?"
    
    &PORTRAIT_nour_nournervous
        
        * ["Yeah. She was from my high school."]
        
        * ["I <i>knew</i> her."]
        
        * ["Does it matter?"]
    
    - &PORTRAIT_nour_nourshadow
    
    NOUR: "..."
    
    &PORTRAIT_rina_rinadeadeye
    
    RINA: "..."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Hello? Random girl that Nour recognizes? Do you see us?"

    &ALLOFF
    
    &PAUSE_1.0
    
    RINA: "You have trapped yourselves in your pasts."
    
    RINA: "It's up to you to move on."
    
    &PAUSE_1.0

    &SHLORP_POOP_Rina_out
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: "..."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Uh, where'd she go?"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "And what the heck was she talking about?"
    
    &EMOTE_NICK_UpIdle
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "...Well, looks like she turned off those weird lights."
    
    &EMOTE_NICK_RightIdle
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "We must be hallucinating. That’s the only explanation I can think of to describe…"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "...Whatever it is that we are seeing."
    
    &PORTRAIT_nour_nourneutral
    
        * "Maybe we hit our heads somewhere... Or a potential gas leak?"
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "That <i>would</i> be somewhat realistic. I want to agree, but..."
            
            &PORTRAIT_ms. suwan_mssuwanneutral
            
            MS. SUWAN: "This place would have been closed off more securely if there were any danger."
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "Any problems would have also been reported to me."
            
            &PORTRAIT_nick_nickpondering
            
            NICK: "I don't remember us hitting our heads anywhere, either!"
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "Well people don't really <i>remember</i> passing out."
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "...Okay, fair." 
            
        * "Maybe this is all just a dream..."
            
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "Haha! Maybe none of us are even real. <i>Ooo~!</i>"
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "Or drugs? This could be a bad trip..."
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "DRUGS?! What kind of drug would make up something like THIS?"
            
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "...Do you have any more?"
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "Enough of your nonsense, please."
    
        * "If this floor was torn down, I don't think we could explain anything of this with logic."
        
            &PORTRAIT_nour_nournervous
            
            NOUR: "Wherever we are... it's definitely not the demolition site from before."
            
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "That's so cool! It's like we warped through some kind of elevator-shaped portal!"
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "I'm not buying any of this." 
            
            MS. SUWAN: "There is no such thing as a supernatural elevator or laminated spaces."
            
            &PORTRAIT_nick_nickangry
            
            NICK: "<i>Liminal<i> spaces."
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            Ms. SUWAN: "..."

    - &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "This is absurd. We need to go back."
    
    &EMOTE_NOUR_UpIdle
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "The elevator is closed, though."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "And it doesn't look like the doors will budge, either."
    
    &EMOTE_SUWAN_UpIdle
    
    &FOLLOW_NICK_NickOuterButtonPos
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Then I'll just press the—"
    
    &EMOTE_NICK_ButtonPress
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "Hey, the buttons are gone!"
    
    &PORTRAIT_nour_noursad

    NOUR: "No use in waiting for an elevator that won't come."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "We should try to find another way out of here."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "I have a map that I was using during my inspection."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "We can reference it if needed."
    
    &EMOTE_NICK_DownIdle
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Well... While we're at it, and since we're already here..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "I wanna see if they still have games in the TopicSpot!"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "<i>That's</i> what you are worried about right now?"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Honestly it's a start. Who knows, maybe we'll be able to ask someone for help there."
    
    Nour hesitates. Nick and Ms. Suwan look at her, expectedly.
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "...Like that girl?"
    
    &PORTRAIT_nour_noursad
    
    NOUR: "..."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "..."
    
    &PORTRAIT_nick_nicksmallsmile
    
    NICK: "We could find her again, and she could tell us."
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: "..."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Uh, Nour? Why aren't you saying anything—"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "We can find our own way out of here."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Okay, then..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Um, sorry. Yeah."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "I... I guess we could talk to her."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>How was Rina even here... ?</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>And she said we TRAPPED OURSELVES in the past?</i>
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>What is there to move on from?</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>...Was that even really her?</i>
    
    &FOLLOW_NICK_NOUR
    
    &FOLLOW_SUWAN_NOUR
    
    &FOLLOW_NOUR_stop
    
    -> END
    
//---------------- MAIN CHOICE BRANCHES ----------------
//All choices are always present, but they need to be unlocked.
//The ELEVATOR choice, however, is only shown once the others are chosen.
//The idea is that the player may repeatedly choose the same choice, expecting a different result.
//eventually, there WILL be a different result.
//I hope this is appropriately frustrating, not annoying.

== rina_branches ==

    &PORTRAIT_rina_rinadeadeye
    
    RINA: "..."
    
    &PORTRAIT_rina_rinasad

    RINA: "I don't know why you're <i>still</i> upset. You're the one who blocked me out..."
    
    &PORTRAIT_rina_rinaangry
    
    RINA: "I <i>tried</i> to keep being your friend!"
    
    &PORTRAIT_nour_nourneutral
    
            //DISPLAY ONCE ALL OTHER CHOICES ARE VISITED
            {
                - rina_final:
                    -> rina_elevator
            }
    
        //this choice always works, and Rina tells Nour to help the people they're with now (Nick and Suwan)
        + ["Friends are supposed to be there for each other!"]
            
            //IF THE LAST KNOTS IN BOTH ROUTES ARE VISITED:
            {
                - nourplaysaplequest && chadbrodykristy3:
                    -> rina_help
            }
            
            &PORTRAIT_nour_nourangry
            
            NOUR: "Friends are supposed to be there for each other, Rina, <i>especially</i> during hard times."
            
            &PORTRAIT_nour_noursad
            
            NOUR: "But you weren't there for me at all on <i>that</i> day."
            
            Rina glances down at Nick, and up to Ms. Suwan.
            
            She returns her attention to Nour.
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "You're still thinking about what happened?"
            
            &PORTRAIT_rina_rinadeadeye
            
            RINA: "...Interesting."
            
            &PORTRAIT_nour_nourangry
            
            NOUR: "Interesting?! Of <i>course</i> I'm still thinking about it."
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "You aren't ready for this — not yet." 
            
            RINA: "<i>They</i> need you right now."
            
            &PORTRAIT_rina_rinaannoyed
            
            RINA: "Come back when you've helped the others first."
            
            &PORTRAIT_rina_rinasad
            
            RINA: "Maybe then you'll know how to help yourself."
        

        + ["I can't do this right now."]
        
            //IF THE LAST KNOT IN NICK'S ROUTE IS VISITED:
            {
                - rina_topicspot:
                    -> rina_topicspot.neverknow
                    
                - nourplaysaplequest:
                    -> rina_topicspot
            }
            
            &PORTRAIT_nour_noursad
            
            NOUR: "I..."
            
            &PORTRAIT_nour_nourshadow
            
            NOUR: "I can't. I'm not ready yet."
            
            &PORTRAIT_nour_noursad
            
            NOUR: "I don't know how to confront what you did to me."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "I get that..." 
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "Why even try things if you know it'll never work out?"
            
            &PORTRAIT_nick_nicksad
            
            NICK: "Giving up on stuff you know you could never succeed?"
            
            &PORTRAIT_nick_nickshadow
            
            NICK: "That just saves you the disappointment."
            
            NICK: "Saves <i>everyone</i> the disappointment."
            
            &PORTRAIT_nour_noursad
            
            NOUR: "..."
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: <i>That felt really personal to Nick. What made him lack self-esteem?</i>
            
            &PORTRAIT_nour_noursad
            
            NOUR: <i>I wonder if I can encourage him to be more confident in himself?</i>
        

        + ["I have to let go of what she did."]
        
            //IF THE LAST KNOT IN SUWAN'S ROUTE IS VISITED:
            {
                - rina_hallway:
                    -> rina_hallway.neverknow
                    
                - chadbrodykristy3:
                    -> rina_hallway
            }
            
            &PORTRAIT_nour_noursad
            
            NOUR: "But I..."
            
            &PORTRAIT_nour_nourshadow
            
            NOUR: "I can't."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "What happened made me so, so angry. And I'm <i>still</i> angry."
            
            &PORTRAIT_nour_nourshadow
            
            NOUR: "I don't know how to let go."
            
            &PORTRAIT_ms. suwan_mssuwanneutral
            
            MS. SUWAN: "Then <i>don't</i>. Why dwell on the past?"
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "You were right back then, and you're still right now."
            
            &PORTRAIT_ms. suwan_mssuwanshadow
            
            MS. SUWAN: "Don't waste your time thinking about it. Forget it entirely."
            
            &PORTRAIT_nour_noursad
            
            NOUR: "..."
            
            &PORTRAIT_nour_nournervous
            
            NOUR: <i>Ms. Suwan seems to be avoiding her past, and is trying to get me to do the same...</i>
            
            NOUR: <i>But that doesn't seem healthy.</i>
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: <i>I'm not sure if I should listen to her.</i>
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: <i>But maybe... I can help her come to terms with her past more?<i>
            
            
        + ["We're done talking for now."]
           
           {
            - rina_bandn && rina_topicspot && rina_hallway:
                -> rina_final
            }
            
            Rina's eyes glaze over.
            
            &PORTRAIT_rina_rinadeadeye
            
            RINA: "..."
            
            &PORTRAIT_nour_noursad
            
            NOUR: <i>I can't go through all the ways I could talk to her again about what happened.</i>
            
            &PORTRAIT_nour_nourshadow
            
            NOUR: <i>I don't think I'm ready yet.</i>
            
            &PORTRAIT_nour_nourneutral
            
            NOUR: <i>I'll just try to help Nick and Ms. Suwan first.</i>
            
            -> END
    
        //"resetting" rina, as if she forgot the choice nour just made.
    -   Rina's eyes glaze over briefly. She returns to her familiar demeanor.
        
        -> rina_branches


== rina_final ==
    
    &PORTRAIT_rina_rinaneutral

    RINA: "You're almost ready to move on from the past..."
    
    RINA: "<i>Almost</i>."
    
    The sound of a power generator starting up can be heard from the hallway.
    
    RINA: "I fixed the elevator that you all broke."
    
    &PORTRAIT_nick_nickshocked
    
    NICK: "How the heck did you know how to do that?"
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "You don't exactly seem old enough to know how to fix an elevator."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Maybe she controls the mall with her <i>magical powers</i>..."
    
    &MOV_RINA_END
    
    &PORTRAIT_nick_nickbigsmile
    
    NICK: "See? Magical."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Well..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "I guess we can get out of here now?"
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "<i>Please.</i> Let's go."
    
    -> END

//---------------- BEGINNING ----------------
//first active conversation with Rina, in Bandn
//This is the big conversation about the incident in the HS cafeteria; it reaches a boiling point.
//The boiling point directs to the branches.

== rina_bandn ==

    &PORTRAIT_nour_nournervous

    NOUR: <i>I don't know how the hell Rina got in here, but regardless...</i>
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>I can't believe we were ever friends.</i>
    
    &PORTRAIT_nour_nournervous
    
        * "I haven't seen you since graduation."
            
            &PORTRAIT_nour_nourannoyed
        
            NOUR: "I wish I could've kept it that way."
            
            &PORTRAIT_rina_rinaannoyed
            
            RINA: "We don't speak for years, and this is how you greet me?"
            
            &PORTRAIT_rina_rinaangry
            
            RINA: "Well, fine. I see how it is."
            
            &PORTRAIT_rina_rinaannoyed
            
            RINA: "Hello to you, too, Yousra."
            
        * "I guess this is what I get for coming back to the mall."
        
            &PORTRAIT_nour_nourannoyed
        
            NOUR: "I was really hoping I wouldn't run into you."
            
            &PORTRAIT_rina_rinaannoyed
    
            RINA: "We don't speak for years, and this is how you greet me?"
            
            &PORTRAIT_rina_rinaangry
            
            RINA: "You know, I'm allowed to exist here just as much as you, Yousra."
            
    -   * "Yousra?"
    
            &PORTRAIT_nour_nournervous
    
            NOUR: "Nobody calls me that anymore. I go by Nour now."
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "Oh... I see."
            
            &PORTRAIT_rina_rinaannoyed
            
            RINA: "...At least you're talking to me again."
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "That's a good start."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "...I can't see what it's a good start <i>to</i>."
            
            &PORTRAIT_rina_rinasad
            
            RINA: "It's because I know you want to keep talking about what happened."
            
            &PORTRAIT_nour_nournervous
            
            NOUR: "..."
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "About <i>why</i> we don't talk anymore."
            
            &PORTRAIT_rina_rinasad
            
            RINA: "About what happened back then? That lunch with our friends..."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "I would prefer not to, but I have a feeling you're going to keep going anyway."
            
            &PORTRAIT_rina_rinaannoyed
            
            RINA: "We started talking about, well, you know..."
            
            &PORTRAIT_rina_rinasad
            
            RINA: "Gay marriage being legalized..."
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "What am I forgetting?"
            
            &PORTRAIT_nour_nournervous
            
                ** "You were quiet[."], while others went on about how being gay is gross and haram."
                
                    &PORTRAIT_rina_rinaannoyed
                
                    RINA: "That doesn't mean I agreed with them."
                    
                    &PORTRAIT_rina_rinaangry
                    
                    RINA: "I didn't even tell them that <i>you're</i> gay!"
                    
                ** "I had <i>just</i> come out to you."
                
                    &PORTRAIT_rina_rinaangry
                
                    RINA: "True, but I didn't tell anyone!"
                
    -   &PORTRAIT_rina_rinaannoyed
    
    RINA: "I didn't tell your secret to <i>anyone</i>. Isn't that what you wanted?"
    
    &PORTRAIT_nour_nourneutral
        
        * "Thanks for that..."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "But that's not the point."
            
            &PORTRAIT_nour_noursad
            
            NOUR: "I'm mad that you didn't even <i>try</i> to disagree with what they were saying."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "You made it seem like you were on their side. Not mine."
        
        * "I know you didn't."
        
            &PORTRAIT_nour_nournervous
        
            NOUR: "Like I said back then, I appreciate you keeping it private."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "I <i>don't</i> appreciate that you didn't even try to disagree with what they were saying."
            
            &PORTRAIT_nour_noursad
            
            NOUR: "You made it seem like you were on their side. Not mine."
        
    -  * "They were quoting everything like parrots..."
    
            &PORTRAT_nour_nourannoyed
    
            NOUR: "Like how they all thought people should keep their bad urges to themselves."
            
            &PORTRAIT_nour_nourangry
    
            NOUR: "That legalizing gay marriage would lead us down a slippery slope!"
            
            &PORTRAIT_nour_nourshadow
            
            NOUR: "To <i>even worse things</i>."
            
            &PORTRAIT_rina_rinaannoyed
                
            RINA: "What did you want <i>me</i> to do about it?"
            
            &PORTRAIT_rina_rinaangry
            
            RINA: "Make everyone suddenly unlearn what our parents taught us?"
            
            &PORTRAIT_rina_rinaannoyed
            
            RINA: "What our Middle-Eastern, Muslim parents taught to our friends?"
            
            &PORTRAIT_rina_rinaangry
            
            RINA: "<i>Especially</i> about queerness?"
            
            &PORTRAIT_nour_noursad
                        
                ** "I wanted you to be my ally..." 
                
                    &PORTRAIT_nour_nourannoyed
                
                    NOUR: "I wanted you to say that you support queer people."
                    
                    &PORTRAIT_nour_nourpondering
                    
                    NOUR: "Or, at least that we're supposed to love all of god's creations."
                    
                    &PORTRAIT_nour_nourannoyed
                
                    NOUR: "Even that would've been better than <i>nothing!</i>"
                    
                    &PORTRAIT_rina_rinaannoyed
                
                    RINA: "If I told them I'm an ally..."
                    
                    &PORTRAIT_rina_rinasad
                    
                    RINA: "They would think I'm supporting something that's <i>haram!</i>"
                    
                    &PORTRAIT_nour_nourshadow
                    
                    NOUR: "... You're worried about <i>yourself?</i>"
                    
                    &PORTRAIT_nour_nourannoyed
                    
                    NOUR: "In front of the person that's <i>actually</i> gay?"
                    
                    &PORTRAIT_nour_nourangry
                    
                    NOUR: "Are you saying <i>I'm</i> haram?"
                    
                    &PORTRAIT_nour_nourannoyed
                    
                    
                ** "I wanted you to disagree with them."
                
                    &PORTRAIT_nour_nourannoyed
                
                    NOUR: "I wanted you to argue that they're just brainwashed by their parents."
                    
                    &PORTRAIT_rina_rinasad
                
                    RINA: "But if I disagreed with them, they would be suspicious of <i>me</i>."
                    
                    &PORTRAIT_nour_nourshadow
                    
                    NOUR: "... You're worried about protecting <i>yourself?</i>"
                    
                    &PORTRAIT_nour_nourangry
                    
                    NOUR: "In front of the person that's <i>actually</i> in danger?"
                    
                    &PORTRAIT_nour_nourannoyed
                
                
    -   * "Well, imagine how I feel!"
    
        &PORTRAIT_nour_nourshadow
    
        NOUR: "I'm the one whose <i>basic rights</i> were being debated."
            
        * "You can still choose to be 'normal' at the end of the day."
        
            &PORTRAIT_nour_nourshadow
        
            NOUR: "But I can't change how I am."
        
    -   &PORTRAIT_rina_rinaannoyed
    
        RINA: "I know that. But I <i>said</i> that I'm sorry."
        
        &PORTRAIT_nour_noursad
    
        NOUR: "You know I wouldn't throw away ten years of friendship."
        
        &PORTRAIT_rina_rinasad
    
        RINA: "But you know what would've happened if I said something then?"
        
        &PORTRAIT_rina_rinaannoyed
        
        RINA: "They would've told their siblings, and their cousins..."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "And then people's parents would find out."
        
        &PORTRAIT_rina_rinaannoyed
        
        RINA: "And all our parents know each other, so they'd <i>all</i> know."
        
        &PORTRAIT_rin_rinaangry
        
        RINA: "Then my parents would get mad, too."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "I can't help that people talk!"
        
        &PORTRAIT_rina_rinaangry
        
        RINA: "I was just looking out for the <i>both</i> of us!"
        
        &PORTRAIT_rina_rinasad
        
        RINA: "I told you, I didn't feel any different about you after you came out..."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "It's not like you could help it. I know that."
        
        &PORTRAIT_rina_rinaannoyed
        
        RINA: "But <i>you</i> were the one who didn't want to hear it."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "I didn't even get a chance to make it up to you."
        
        &PORTRAIT_nour_nourannoyed
 
            * "It's too late. The damage is done."
            
                &PORTRAIT_nour_nourangry
            
                NOUR: "I knew if my best friend couldn't stand up for me..."
                
                &PORTRAIT_nour_nourshadow
                
                NOUR: "It would be a long time before <i>anyone</i> would."
            
            * "How was I supposed to know you were telling the truth?"
            
                &PORTRAIT_nour_nourangry
            
                NOUR: "My parents told me nothing would change how they feel about me, too."
                
                &PORTRAIT_nour_nourshadow
                
                NOUR: "I knew that being gay would prove that wrong."
            
            * "You still don't get it, do you?"
            
                &PORTRAIT_nour_nourangry
            
                NOUR: "You can't just pick and choose when to be a good friend."
                
    - -> rina_branches


//---------------- TOPICSPOT ----------------
//At the end of Nick's route, he gains confidence in himself, and realizes he isn't stupid.
//Nour takes this to mean that she should be confident, but she's OVER confident as a result.

== rina_topicspot ==

    &PORTRAIT_nour_nournervous
    
    NOUR: <i>Like Nick said... I can do it.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>I helped him be more confident. I should be the same way.</i>
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: <i>I'll show her confidence, alright.</i>

    &PORTRAIT_nour_nourannoyed

    NOUR: "Why would I want to keep being friends with you?"
    
    &PORTRAIT_nour_nourangry
    
    NOUR: "A <i>coward</i> who couldn't stand up for me?"
    
    &PORTRAIT_rina_rinaannoyed
    
    RINA: "You can call me a coward. I don't care."
    
    &PORTRAIT_rina_rinasad
    
    RINA: "But don't you get where I'm coming from?"
    
    &PORTRAIT_nour_nourannoyed
    
        * "You're not the victim here."
        
            &PORTRAIT_nour_noursad
        
            NOUR: "You just watched our 'friends' say all of that stuff about people like me."
            
            &PORTRAIT_nour_nourannoyed
            
            NOUR: "You <i>enabled</i> them."
            
            &PORTRAIT_rina_rinaneutral
        
            RINA: "I know I'm not the victim, but you have to admit..."
            
            &PORTRAIT_rina_rinasad
            
            RINA: "They would have hated <i>both</i> of us if I said something."
        
        * "I get that you don't have principles."
        
            &PORTRAIT_nour_nourangry
        
            NOUR: "You don't believe in anything enough to fight for it."
            
            &PORTRAIT_rina_rinaangry
    
            RINA: "I <i>do</i> believe that queer people deserve respect."
            
            &PORTRAIT_rina_rinaneutral
            
            RINA: "But I had to play it smart."
            
            &PORTRAIT_rina_rinasad
            
            RINA: "I knew they would hate <i>both</i> of us if I said something."
        
    -   &PORTRAIT_rina_rinaannoyed
    
        RINA: "You know, I'm not the bad guy here."
        
        &PORTRAIT_rina_rinaangry
    
        RINA: "I'm not the one who said what they said."
        
        &PORTRAIT_nour_nournervous
        
            * "You might as well have."
            
                &PORTRAIT_nour_nourannoyed
            
                NOUR: "Being complacent is the same thing as agreeing."
            
            * "I know..."
            
                &PORTRAIT_nour_nourannoyed
            
                NOUR: "You just stayed neutral. Not the best or the worst response."
            
    -   &PORTRAIT_rina_rinaneutral
    
        RINA: "You're speaking up now because you wish I had back then. Right?"
        
        &PORTRAIT_rina_rinasad
        
        RINA: "But I'm not the person you really want to confront."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "You want our homophobic friends to know their place."
        
        &PORTRAIT_rina_rinaannoyed
        
        RINA: "You closed yourself to any possiblity for me to support you."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "We were both afraid and inexperienced. But I could've learned..."
        
        &PORTRAIT_rina_rinadeadeye
        
        RINA: "Instead of becoming collateral."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "Did you have so little faith in me?"
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "That's not true! I..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "..."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "And now look."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "You've hurt <i>both</i> of us this way."
        
        &PORTRAIT_nour_nourshadow
        
        NOUR: "..."
        
            -> neverknow
    
    = neverknow
    
        &PORTRAIT_rina_rinaneutral
    
        RINA: "Nour, <i>neither</i> of us had the guts to say anything back then."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "You'll never know what would have happened if I spoke up."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "And even if either of us <i>did</i>..."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "I hope <i>this</i> isn't the way you wish our conversation had gone."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "..."
        
        &PORTRAIT_nick_nicknervous
        
        NICK: "Nour..."
        
        &PORTRAIT_nour_nourshadow
        
        NOUR: <i>She's right. This way was no good.</i>
        
        &PORTRAIT_nour_nournervous
        
        NOUR: <i>I was way too aggressive. I would've only push her away further.</i>
        
        &PORTRAIT_nour_noursad
        
        NOUR: <i>Acting this way would've just burn her and myself.</i>
        
        &PORTRAIT_nour_nourshadow
        
        NOUR: <i>... Looks like this wasn't the right way to handle what happened.</i>
        
        { - !neverknowlight:
                -> neverknowlight
            
            - else: 
                -> END
        }
        
    = neverknowlight
    
        &LIGHT_Nour0
    
        -> light_on

//---------------- HALLWAY ----------------
//At the end of Ms. Suwan's route, Ms. Suwan is able to apologize for her past behavior and let go.
//Nour takes this to mean that she should let things slide, and she ends up being too passive as a result.

== rina_hallway ==

    &PORTRAIT_nour_nournervous
    
    NOUR: <i>Okay. I can do this.</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>I helped Ms. Suwan let go of her broken friendship. Maybe I should, too.</i>
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: <i>Just completely forget and let go. Like it never happened...</i>

    &PORTRAIT_nour_noursad
    
    NOUR: "Whatever. You know what? I'm not mad anymore."
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: "It's fine. I don't know why I was so upset."
    
    &PORTRAIT_rina_rinaannoyed
    
    RINA: "Really? Because a moment ago, you were huffing about how much I hurt you."
    
    &PORTRAIT_rina_rinaangry
    
    RINA: "Like, how it's my fault that it's hard for you to trust anyone now."
    
    &PORTRAIT_nour_nournervous
    
        * "Nope, forget it.["] I'm moving on now."
        
            &PORTRAIT_rina_rinaannoyed
        
            RINA: "You're gonna move on, just like that? After ignoring me for three years?"
            
            &PORTRAIT_nour_noursad
            
            ** "Well, yeah.["] Clearly, you're not going to admit that you were wrong.
            
            -> admitwrong
            
            ** "I was hurt, so I needed time.["] Time heals all wounds, or something..."
            
            -> hurtme
        
        * "Sometimes, people who care about you will hurt you."
        
            &PORTRAIT_nour_noursad
        
            NOUR: "It's just a part of life, I've accepted that now."
            
            &PORTRAIT_rina_rinaannoyed
        
            RINA: "I see. So you're <i>really</i> okay with expecting me to hurt you again?"
            
            &PORTRAIT_nour_noursad
            
            ** "I guess I am.["] I should always expect those I love to hurt me."
            
            -> hurtme
            
            ** "So you admit that you hurt me?["] You admit that you were wrong?"
            
            -> admitwrong
    
    = admitwrong
    
        &PORTRAIT_rina_rinaangry
    
        RINA: "That's all you want? For me to admit that I was wrong?"
        
        &PORTRAIT_rina_rinaannoyed
        
        RINA: "Fine, I was wrong. I admit it. And I wish I didn't hurt you."
        
        -> toolate
        
    = hurtme
    
        &PORTRAIT_rina_rinaannoyed
        
        RINA: "I know I hurt you... I'll admit, I was wrong."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "... And I can't take back what I did."
        
        -> toolate
        
    = toolate
    
        &PORTRAIT_nour_nourannoyed
        
        NOUR: "Whatever. The people I care about, trust, and open up to..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "They're all bound to hurt me, one way or the other."
        
        &PORTRAIT_nour_nourshadow
        
        NOUR: "I've accepted that."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "Rina?"
        
        &PORTRAIT_rina_rinasad
        
        RINA: "I hope <i>this</i> isn't the way you wish our conversation had gone."
    
        &PORTRAIT_rina_rinaneutral
    
        RINA: "But it's too late for that. You closed the door on me, right?"
        
        &PORTRAIT_rina_rinadeadeye
        
        RINA: "I didn't <i>actually</i> get the chance to say any of this."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "It's too late for this resolution."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "You couldn't <i>possibly</i> think forgetting everything..."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "Would be a realistic solution? Because, I mean..."
        
        &PORTRAIT_rina_rinadeadeye
        
        RINA: "<i>You still think about me no matter how hard you try not to.</i>"
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "And passively accepting that the people you care about will <i>always</i> hurt you..."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "Do you really want to live your life that way?"
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "Well—!"
        
        &PORTRAIT_nour_noursad
        
        NOUR: "..."
        
        &PORTRAIT_ms. suwan_mssuwansad
        
        MS. SUWAN: "Nour..."
        
        //NOTE It would be nice to incorporate consoling from Ms. Suwan here
        
        -> neverknow
        
    = neverknow
    
        &PORTRAIT_rina_rinaneutral
    
        RINA: "You missed your chance to forgive me, Nour."
        
        &PORTRAIT_rina_rinadeadeye
        
        RINA: "You'll never know what would have happened if you forgave me."
        
        &PORTRAIT_nour_nournervous
        
        NOUR: <i>That was too avoidant of me. I can't passively accept all the pain like that...</i>
        
        &PORTRAIT_nour_noursad
        
        NOUR: <i>To the point of ignoring everything that happened.</i>
        
        &PORTRAIT_nour_nourshadow
        
        NOUR: <i>... Looks like this wasn't the right way to handle what happened.</i>
        
        { - !neverknowlight:
                -> neverknowlight
            
            - else: 
                -> END
        }
        
    = neverknowlight
        
        &LIGHT_Nour1

        -> light_on


//---------------- ELEVATOR ----------------
//she's blocking the elevator; end of nour's route

== rina_elevator ==


    //umm assuming that she's blocking the elevator, which is now working...
    //that should probably be mentioned here but im tired of this rn so do it later
    
    //Ms suwan can look back at the past now
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "Nour, think back to when you were friends with Rina."
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "Who was she to you?"
    
    &PORTRAIT_nour_noursad
    
    * "Rina, you were[..."] the only person who understood me."
        
        &PORTRAIT_rina_rinasad
        
        RINA: "..."
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "Nour, you helped me with my old friends."
        
        &PORTRAIT_ms. suwan_mssuwanneutral
        
        MS. SUWAN: "You helped me tell them what I wish I could have told them sooner."
        
        &PORTRAIT_ms. suwan_mssuwanpondering
    
        MS. SUWAN: "You've already told Rina what you <i>wish</i> you could've said..."
        
        &PORTRAIT_ms. suwan_mssuwansad
        
        MS. SUWAN: "And it isn't working for you."
        
        &PORTRAIT_ms. suwan_mssuwanneutral
        
        MS. SUWAN: "You need to face the past, Rina... <i>everything.</i>"
        
        &PORTRAIT_ms. suwan_mssuwanpondering
        
        NICK: "You've helped us. Now it's our turn to help <i>you</i>."
        
        &PORTRAIT_nick_nickshocked
        
        NICK: "Ms. Suwan's right, Nour!"
        
        &PORTRAIT_nick_nicknervous
        
        NICK: "We'll be behind you every step of the way, too."
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "..."
        
        &PORTRAIT_ms. suwan_mssuwanneutral
         
        MS. SUWAN: "Nour..."
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "What <i>really</i> happened?"
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "Rina..."
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "I remember what I told you, when I first came out..."
        
        &PORTRAIT_nour_nourneutral
        
            ** "My family didn't help me feel comfortable."
            
                &PORTRAIT_nour_nourshadow
            
                NOUR: "They'd make backhanded comments about how homosexuality is haram."
                
                &PORTRAIT_nour_nourangry
                
                NOUR: "How it was a terrible 'choice' to make."
                
                &PORTRAIT_nour_nourannoyed
            
                NOUR: "Our friends showed how they felt in that cafeteria discussion."
                
                &PORTRAIT_nour_nournervous
                
                NOUR: "But I knew you would never say those things to me."
                
                &PORTRAIT_nour_noursad
                
                NOUR: "I always felt comfortable around you."
            
            ** "I needed to feel like I wasn't alone."
            
                &PORTRAIT_nour_nournervous
            
                NOUR: "I couldn't share how I felt with my family..."
                
                &PORTRAIT_nour_nourshadow
                
                NOUR: "Because they'd warn me that acting on it is haram."
                
                &PORTRAIT_nour_nourannoyed
            
                NOUR: "I couldn't tell any other friends, point proven by that cafeteria discussion."
                
                &PORTRAIT_nour_nournervous
                
                NOUR: "But I knew I had you because you were always there for me."
                
                &PORTRAIT_nour_noursad
                
                NOUR: "You made sure I was never alone."
    
    -   &PORTRAIT_nour_nourannoyed
    
    NOUR: "And when I felt like <i>you</i> weren't looking out for me..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "..."
    
    &PORTRAIT_nour_nourshadow
    
    NOUR: "I had nobody to go to."
    
        &PORTRAIT_nick_nicknervous
        
        //nick isn't as afraid of being a disappointment anymore
        NICK: "Why'd you <i>really</i> stop talking to Rina?"
        
        &PORTRAIT_nick_nicksad
        
        NICK: "Is it actually about her saying nothing?"
        
        &PORTRAIT_nour_noursad
        
            * "I'm scared of people turning on me."
            
                &PORTRAIT_nour_nourshadow
            
                NOUR: "If I left you first, you wouldn't even have a chance to make me feel bad."
            
            * "I couldn't bring you down with me."
            
                &PORTRAIT_nour_nournervous
            
                NOUR: "I guess I felt like you were better off not being associated with me."
                
                &PORTRAIT_nour_nourshadow
                
                NOUR: "It seemed like you would have been happier that way."
            
    -   &PORTRAIT_nour_noursad
    
        NOUR: "When I say it out loud, it feels silly."
        
        &PORTRAIT_nour_nourtears
        
        NOUR: "I'm still mad at you, but I don't think I <i>want</i> to be."
        
        &PORTRAIT_nick_nicknervous
        
        NICK: "You know, I do the stuff I do because..."
        
        &PORTRAIT_nick_nicksad
        
        NICK: "I guess because I'm frustrated at the world."
        
        &PORTRAIT_nick_nickshocked
        
        NICK: "But I'm actually mad at myself, 'cause I can't be how people want me to be."
        
        &PORTRAIT_nick_nicksad
    
        NICK: "So like, you know..."
        
        &PORTRAIT_nour_noursad
        
            * "I can't change Rina."
            
                &PORTRAIT_nour_nournervous
            
                NOUR: "I'm mad that she didn't say anything..."
                
                &PORTRAIT_nour_nourshadow
                
                NOUR: "But I'm also mad that <i>I</i> didn't say anything."
            
            * "I can't take back my own choices."
            
                &PORTRAIT_nour_nournervous
            
                NOUR: "I already made them. I'm <i>really</i> mad at myself for doing what I did."
                
                &PORTRAIT_nour_nourshadow
                
                NOUR: "Maybe even more than I was mad at Rina."
            
        - &PORTRAIT_nour_noursad
        
        NOUR: "And it all came back to me feeling like running away was a better choice."
        
        &PORTRAIT_nour_nourshadow
        
        NOUR: "... And I can't change what Rina did."
        
        &PORTRAIT_nour_nourtears
        
        NOUR: "I can't change what <i>I</i> did. It's in the past."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "It's up to both of us to move forward."
        
        &PORTRAIT_ms. suwan_mssuwansad
        
        MS. SUWAN: "Well said, Nour."
        
        &PORTRAIT_nick_nicksad
        
        NICK: "And we're here now, whether you like it or not."
        
        &PORTRAIT_nick_nickbigsmile
        
        NICK: "Mostly 'cause we're just stuck here together."
        
        &PORTRAIT_ms. suwan_mssuwansmile
        
        MS. SUWAN: "Correct, because <i>Rina</i> here is blocking the way."
        
        &PORTRAIT_nick_nickshocked
        
        NICK: "Oh, yeah, move it!"
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "Nour..."
        
        &PORTRAIT_rina_rinasadsmile
        
        RINA: "Are you ready to move on?"
        
        &PORTRAIT_nour_nourtears
        
        *   "With or without you..."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "I'm ready to move on."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "Without."
        
        &PORTRAIT_rina_rinasadsmile
        
        RINA: "I'm not real, you know..." //maybe it's too silly idk NO IT IS NOT <3 LOl
        
        &PORTRAIT_nour_noursad
        
        NOUR: "You're in the past now."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "..."
        
        &PORTRAIT_rina_rinasadsmile
        
        RINA: "Goodbye, Nour."
        
        &PAUSE_0.5
        
        &MOV_RINA_VOID
        
        &PAUSE_0.5
        
        &LIGHT_Nour2
        
        &PAUSE_0.5
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "..."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "Rina..."
        
        &PORTRAIT_nour_nourtears
        
        NOUR: "Goodbye, old friend."
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "..."
        
        &PORTRAIT_nick_nicknervous
        
        NICK: "..."
        
        &PORTRAIT_ms. suwan_mssuwansmile
        
        MS. SUWAN: "You did well, Nour."
        
        &PORTRAIT_nick_nicksmallsmile
        
        NICK: "Yeah. We're proud of you."
        
        //maybe staging here. ms. suwan and nick gather around nour. huddle/hug moment
        
        &PORTRAIT_nick_nickpondering
        
        NICK: "Now, shall we move on? Because <i>I</i> believe it is..."
        
        &PORTRAIT_nick_nickbigsmile
        
        NICK: "<i>Button smashing time!</i>"
        
        &PORTRAIT_ms. suwan_mssuwannervous
        
        MS. SUWAN: "Please don't break the elevator again."
        
        //more staging? suwan and nick walk towards elevator
        
        &PORTRAIT_nick_nickshocked
        
        NICK: "... Nour! Are your coming or what?"
        
        &PORTRAIT_ms. suwan_mssuwansmile
        
        MS. SUWAN: "Ready to leave?"
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "..."
        
        &PORTRAIT_nour_noursmile
        
        NOUR: "Yeah. Let's go."
        
        THEY WALK INTO THE ELEVATOR
        
        -> END


//---------------- HELP ----------------
//Guidance for the player; Rina tells nour to help the others
== rina_help ==

    //If this knot is already visited, display a TLDR here and end scene.
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Friends are supposed to help each other."
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: "You weren't helping me that day, but..."
    
    &PORTRAIT_rina_rinaneutral
    
    RINA: "Seems like you're helping your new companions."
    
    &PORTRAIT_rina_rinasadsmile
    
    RINA: "Would you consider them to be your friends?"
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "..."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Maybe I'm ready to explore all possible outcomes from talking to Rina.</i>
    
    NOUR: <i>I'll try to talk to her again.</i>
    
    -> END


== rina_gotopolaroid ==

    {- !first:
        -> first
        
        - else:
            -> repeat
    }

    = first
    
        &PORTRAIT_nour_noursad
    
        NOUR: "I'm being told that I'm ready to confront you, but..."
        
        &PORTRAIT_nour_nournervous
        
        NOUR: "I don't want to revisit this."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "Are you sure?"
        
        &PORTRAIT_rina_rinasad
        
        RINA: "..."
        
        &PORTRAIT_rina_rinaneutral
        
        RINA: "Nour, don't you remember all the pictures we took together?"
        
        &PORTRAIT_nour_nourrealization
        
        NOUR: "... What?"
        
        RINA: "Before you became a pro photographer."
        
        &PORTRAIT_rina_rinasadsmile
        
        RINA: "When we were just two kids with a camera, capturing memories."
        
        &PORTRAIT_nour_noursad
        
        NOUR: "..."
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: <i>With the polaroid camera that my dad got...</i>
        
        NOUR: <i>... from this mall.</i>
        
        -> END
    
    = repeat
    
        &PORTRAIT_nour_nourpondering
        
        NOUR: <i>Rina wants me to remember when we took pictures together.</i>
        
        &PORTRAIT_nour_nourneutral
        
        NOUR: <i>We used a polaroid camera that my dad got from this mall.</i>
        
        &PORTRAIT_nour_nourpondering
        
        NOUR: <i>... I wonder if I can still find it around her?</i>
    
        -> END
