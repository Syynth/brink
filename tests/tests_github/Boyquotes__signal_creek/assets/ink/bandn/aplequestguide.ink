
== aplequestguide ==

A tutorial guide to an old fantasy videogame called <i>"Aple Quest/: The Space Prince Returns"</i>.

{
    - nourplaysaplequest:
        -> explored_everything
        
    - consoles_start_aplequest:
        -> aplequestguide_after_start_aplequest
        
    - aplequestguide_after_bookshelf_fightstarts:
        -> aplequestguide1_repeat
        
    - nour_suggests_hobbies:
        -> aplequestguide_after_bookshelf_fightstarts
        
    - else:
        -> END
}

== aplequestguide_after_bookshelf_fightstarts ==

    &FOLLOW_SUWAN_SuwanAGPos
    &FOLLOW_NICK_NickAGPos
    &FOLLOW_NOUR_NourAGPos
    &PAUSE_1.0
    &EMOTE_NOUR_LeftIdle
    &EMOTE_SUWAN_RightIdle
    &EMOTE_NICK_DownIdle

    &PORTRAIT_nour_noursmile

    NOUR: "Hey, look what I found!"
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Isn't this the game we saw at TopicSpot?"
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Yeah..."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "Wanna take a look?"
    
    &PORTRAIT_nick_nicksad
    
    NICK: "I don't know..."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "Well, <i>I'm</i> curious, since I know nothing about it. You're welcome to join."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "...Sure, whatever."
    
    &PORTRAIT_nour_nourneutral
    
        * "Here's the author blurb..."
        
            &PORTRAIT_nour_nourpondering
        
            NOUR: "<i>Written by Anthony Appleton, lover of apples and ex-NASA astronaut.</i>"
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "An <i>ex-astronaut</i> wrote for a childish game like this?"
            
            &EMOTE_NICK_Vibrating
            
            &PORTRAIT_nick_nickangry
            
            NICK: "This game isn't childish!"
            
            &PAUSE_0.4
            &EMOTE_NICK_DownIdle
            
            &PORTRAIT_nick_nickneutral
            
            NICK: "It's like, one of those allegories for reaching the final frontier."
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "American space exploration, fear of the unknown, scientific advancements!"
            
            &PORTRAIT_nick_nickpondering
            
            NICK: "Fierce competition with rising nations..."
            
            &PORTRAIT_nick_nickangry
            
            NICK: "All of it inspired the game!"
            
            &PORTRAIT_nick_nicknervous
            
            NICK: "There's intergalactic political coups and unstable regimes based on real life."
            
            &PORTRAIT_nick_nicksad
            
            NICK: "That's why the Aple Prince is stranded on Earth!"
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "...Interesting."
    
        * "The publishing page says..."
            
            &PORTRAIT_nour_nourpondering
        
            NOUR: "<i>Edition 1, 1988.</i>"
            
            &PORTRAIT_ms. suwan_mssuwanpondering
            
            MS. SUWAN: "What is an old edition like this doing here?"
            
            &PORTRAIT_nick_nickshocked
            
            NICK: "Wait, did you just say Edition 1?"
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "This is one of those vintage game guides!"
            
            &PORTRAIT_nick_nickbigsmile
            
            NICK: "They're impossible to find because they only released prints in cereal boxes!"
            
            &PORTRAIT_nour_nourpondering
            
            NOUR: "There was <i>Aple Quest</i> cereal?"
            
            &PORTRAIT_nick_nicksmallsmile
            
            NICK: "Yeah! <i>Aple Space-Os, the nutrionally balanced UFOs!</i>" 
            
            &PORTRAIT_nick_nickpondering
            
            NICK: "The food venture for <i>Aple Quest</i> was a flop. It was discontinued."
            
            &PORTRAIT_ms. suwan_mssuwannervous
            
            MS. SUWAN: "You're telling me this 'official' guide was a cereal freebie?"
            
            &PORTRAIT_nick_nickneutral
            
            NICK: "Yup. Merchandise expansion was still the wild, wild west back then..."
        
        * "It looks like they mispelled apple."
        
            &PORTRAIT_nour_nourpondering
        
            NOUR: "They wrote 'Aple' instead. You know, without another 'p'?"
        
            &PORTRAIT_nick_nickneutral
        
            NICK: "That's intentional."
            
            &PORTRAIT_nick_nickpondering
            
            NICK: "The characters aren't just regular old <i>apples</i>—they're sentient fruit aliens."
            
            &PORTRAIT_ms. suwan_mssuwanangry
            
            MS. SUWAN: "Regardless, I cannot imagine what person would name something <i>Aple Quest</i>."
            
            &PORTRAIT_nick_nickangry
            
            NICK: "Okay... First of all, it's actually a fantastic series with a really good story."
            
            &PORTRAIT_nick_nickneutral
            
            NICK: "And the game mechanics are innovative for its time."
            
            &PORTRAIT_nick_nickangry
            
            NICK: "Second, APLE is actually an acronym."
            
            &PORTRAIT_nick_nickpondering
            
            NICK: "It stands for <i>Apple Prince Leaves Earth</i>. That's the whole basis of the game."
            
            &PORTRAIT_nick_nickangry
            
            NICK: "And third, the 'a' is pronounced like the word 'say', not 'sat'."
            
            &PORTRAIT_nick_nickneutral
            
            NICK: "It's A-ple, not AH-ple."
    
    - &PORTRAIT_nour_nourrealization
    
    NOUR: "Huh... looks like you know a lot about this game after all."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "Um. I don't know."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "What? You clearly know a LOT about this game."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "Let's just pick a random page of the guide."
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Maybe this thing has something worth looking at..."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Or not. I don't care that much."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "I'll choose a page for us to read."
    
        * "Let's look at the first page."
        
        * "Let's look somewhere in the middle."
        
        * "Let's see the end section."
    
    - NOUR: "Hmm... it looks like tips on how to fight the enemies."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "This doesn't make a lot of sense to me. Maybe I don't know the game well enough?"
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Nonsense. A guide is for beginners— let me read it."
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "..."
    
    &PORTRAIT_ms. suwan_mssuwannervous
    
    MS. SUWAN: "This is rather incomprehensible."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "I'll take a look."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "... Huh?!"
    
    &EMOTE_NICK_Jump
    
    &PORTRAIT_nick_nickangry
    
    NICK: "What the— this advice is BOGUS!"
    
    &EMOTE_NICK_DownIdle
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: "You... you understand what's written here?"
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "God, I wish I didn't! This stupid guy is getting everything wrong!"
    
    &PORTRAIT_nick_nickangry
    
    NICK: "Dashing while doing your special Y-attack doesn't work!"
    
    &PORTRAIT_nick_nickpondering
    
    NICK: "Only a special class of Fruit can do that."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "You have to be a member of the royal family Aple tree!"
    
    &PORTRAIT_nick_nickangry
    
    NICK: "And there's only ONE way to unlock royal characters after your first run."
    
    &PORTRAIT_nick_nickneutral
    
    NICK: "You gotta defend the Seedling Vault the entire night of the tutorial."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "The organization of this section <i>stinks</i>. None of it makes sense!"
    
    &PORTRAIT_ms. suwan_mssuwanpondering
    
    MS. SUWAN: "You know a LOT about this game."
    
    &PORTRAIT_ms. suwan_mssuwanannoyed
    
    MS. SUWAN: "Is this all you do in your free time?"
    
    &PORTRAIT_nick_nicksad
    
    NICK: "..."
    
    &PORTRAIT_nour_nournervous
    
    NOUR: "Um, it seems really cool, Nick."
    
    &PORTRAIT_nour_noursmile
    
    NOUR: "I'll definitely check it out when we get back."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "You should spend less time playing childish <i>games</i> and focus more on your future."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "This isn't a childish game. It actually has a large following of <i>all</i> ages."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I worry that kids these days get too caught up in video games."
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "... And aren't contributing enough to things that will matter later on."
    
    &PORTRAIT_ms. suwan_mssuwansad //not ideal emotion but portrait matches
    
    MS. SUWAN: "That's all."
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: "But you <i>obviously</i> played games when you were Nick's age right?"
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "When I was his age, I was already working to help my family."
    
    &PORTRAIT_ms. suwan_mssuwansad
    
    MS. SUWAN: "I had no time to play games."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "But that's you! You <i>DON'T</i> even know my story!"
    
    &PORTRAIT_nour_nourshocked
    
    NOUR: "Nick, let's calm down a little."
    
    &PORTRAIT_nick_nickangry
    
    NICK: "NO! I'm so TIRED of people saying I'm lazy."
    
    &PORTRAIT_nick_nicknervous
    
    NICK: "I'm tired of people comparing me to themselves and other people."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "Like, I get it. I'm not <i>perfect</i>, but that doesn't mean I'm not trying my best."
    
    &PORTRAIT_nour_noursad
    
        * "No one is saying those things."
    
        * "We're not judging you Nick."
    
        * "You're overreacting."
    
    - &PORTRAIT_nick_nickangry
    
    NICK: "I DON'T CARE WHAT YOU HAVE TO SAY! I KNOW WHAT YOU'RE THINKING!"
    
    Nick averts Nour's gaze, visibly restraining himself.
    
    * ["We're here for you."]
    
        NOUR: "We're here-"
    
    * ["What do you want us to do for you?"]
    
        NOUR: "What do you-"
        
    * ["We didn't mean to upset you..."]
    
        NOUR: "We didn't mean-"
        
    - Nick throws up his middle finger.
    
    &EMOTE_NICK_MiddleFinger
    
    NICK: "SCREW"
    
    NICK: "YOU!"
    
    On his wrist is <b>an unfamiliar black bracelet with metallic studs</b> that shine under the light.
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>Wait... I don't recognize that bracelet on him.</i>
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>Is that a price tag on the end of it?</i>
    
    &PORTRAIT_ms. suwan_mssuwanneutral
    
    MS. SUWAN: "... Well, isn't this just nice."
    
        * "Ms. Suwan, do you recognize that bracelet?"
            
            &PORTRAIT_ms. suwan_mssuwanannoyed
            
            MS. SUWAN: "Not on Nick."
        
        * "Nick, where'd you get that bracelet?"
        
        * "..."
    
    - &PORTRAIT_nick_nicknervous 
    
    NICK: "..."
    
    &EMOTE_NICK_LookAround
    &PAUSE_1
    &EMOTE_NICK_DownIdle
    
    Nick quickly retracts his hand, hiding it behind his back.
    
    &PORTRAIT_ms. suwan_mssuwanangry
    
    MS. SUWAN: "You want to talk about maturity while wearing a bracelet that you <i>stole</i>."
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: "Lay off him for a sec."
    
    &PORTRAIT_nour_nourannoyed
    
        * "Nick, be honest. Did you steal that bracelet?"
        
        * "Why did you steal the bracelet, Nick?"
    
    - &PORTRAIT_nick_nicknervous
    
    NICK: I'm not talking to you two anymore. About all this stuff."
    
    &PORTRAIT_nick_nicksad
    
    NICK: "All you do is talk down to me. I'm done listening."
    
    &PORTRAIT_nour_noursad
    
        * "We just want to know the truth..."
        
        * "All you need to do is return the bracelet..."
    
    -  &PORTRAIT_nick_nickshadow
    
    NICK: Nick shakes his head.

    &PORTRAIT_nour_nourneutral

    NOUR: "C'mon, Nick. We're not judging you."
    
    NOUR: "We just want to understand why you're acting like this."
    
    &PORTRAIT_nick_nickshadow
    
    NICK: "..."
    
    &PORTRAIT_nour_noursad
    
    NOUR: "Nick?"
    
    &EMOTE_NICK_LookAround
    &PAUSE_1
    &EMOTE_NICK_UpIdle
    
    He looks away and keeps his mouth shut. 
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>I can't believe he's acting out this way.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>I know that school and the future are hard things to grapple with...</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>... But this feels a bit too personal.</i>
    
    NOUR: <i>I feel bad. Trying to make things better only made things worse.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>How can I get him to talk again?</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>... Maybe I have to speak his language.</i>
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>Books clearly aren't the way to go.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>I should find something that will speak to him. Something to boost his confidence.</i>
    
    &PORTRAIT_nour_nourneutral
    
    NOUR: <i>Something he's good at, knows a lot about, and makes him feel smart.</i>
    
    &LIGHT_Nick1
    &FOLLOW_NOUR_stop
    
    -> light_on
    
== aplequestguide1_repeat ==

    &PORTRAIT_nour_nourneutral

    NOUR: <i>I need to get Nick to trust me again but how?</i>
    
    &PORTRAIT_nour_noursad
    
    NOUR: <i>He won't even look at these books anymore.</i>
    
    &PORTRAIT_nour_nourannoyed
    
    NOUR: <i>Plus, Ms. Suwan wasn't helping the situation at all.</i>
    
    &PORTRAIT_nour_nournervous
    
    NOUR: <i>We really need to get along if we're gonna find a way out of here.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>... He did know an awful lot about that video game.</i>
    
    &PORTRAIT_nour_nourrealization
    
    NOUR: <i> Maybe I could convince him to play the game with me... and Ms. Suwan, too.</i>
    
    &PORTRAIT_nour_nourpondering
    
    NOUR: <i>Wasn't the game set up somewhere in the TopicSpot? I could go look for it...</i>
    
    &FOLLOW_NOUR_stop
    
        -> END
        
== aplequestguide_after_start_aplequest ==

    &PORTRAIT_nour_nourneutral

    NOUR: <i>Nick said this was useless before.</i>
    
    NOUR: <i>I should try searching the hallway myself and not rely on this guide.</i>
    
    // NOUR: <i>If I ever need to keep track of how many apples I've found so far, I can always talk to Emperor Gala back at the TopicSpot consoles.</i>
    
        -> END

// === aplequestguide_end ===

//     &PORTRAIT_nour_nourneutral

//     NOUR: <i>I think we've explored everything we could here.</i>

//         -> END
