== bookstack ==

    {
        - currentWorld == "dream" && bookstack_real:
            -> bookstack_realvisited
            
        - currentWorld == "dream":
            -> bookstack_dream
            
        - currentWorld == "real" && bookstack_dream:
            -> bookstack_dreamvisited
        
        - else:
            -> bookstack_real
    }
//---------------- DREAM WORLD ----------------

== bookstack_dream ==

    + { sailormoonposters_dream } -> sailormooncomic

    NICK: Whoa! It's the first twenty volumes of Blood Titan Online! Oh, man,  that's awesome...! Whoa, look at this cool art!
    
    NOUR: Blood Titan Online? Wasn't that super famous a couple years ago?
    
    NICK: Yeah! I've been looking for this series ever since I watched the anime! The first season left off on such a cliffhanger. And it's been years and they still haven't made season two yet... I've been meaning to get into the manga, but I've never had the chance.
    
    NOUR: Really? Ha. I bet Ms. Suwan could read that whole stack in less than a day. I bet one of those lawyer textbooks has more words in it than that entire series.
    
    MS SUWAN: Ha ha. I suppose I certainly could, if I put my mind to it. What's this Blood... Blood Titan Online about?
    
    NICK: Oh, it's about how - like - there's a video game people get trapped in. And when they die in the game, they die in real life. They fight these monsters all the time, and in order to escape the game they have to defeat the Blood Titan. But first they have to find who the Blood Titan is...
    
    NICK: What makes this series really cool is that anyone could be the Blood Titan. You have to really think and analyze all the hints about each character! 
    
    NOUR: Huh... so it's a mystery thriller?
    
    NICK: Sort of, yeah! Except add in battles, dragons, swords, and magic!
    
    NOUR: Y'know, I do like some mystery, myself. Not a huge comics fan, though. 
    
    MS SUWAN: It's an... interesting premise. Perhaps I'll look into it. I suppose some light reading between work obligations helps to clear my head sometimes.
    
    NICK: Really?! Guys, you should totally give it a chance! We could read it together! It's super awesome, I promise!
    
    -> END


== bookstack_realvisited ==

    NICK: YES! Blood Titan Online is back! Oh, man... I really hope Hirama Joe and the Blood Titan team up. Wouldn't that be so epic? Especially after what happened with Yuno in the Chimera Planet arc... I think the anime left off around volume fifteen--
    
    NICK: Oh! Sorry! No spoilers! I forgot you guys haven't watched the anime...
    
    NOUR: Pfft. It's alright, I don't really mind.
    
    NICK: Okay! Well - I'm gonna take volume fifteen with me. I don't want it to all change into those normie books again. Who knows when I'm gonna find this again, right? 
    
    Nick picks up volume fifteen, spending a moment gazing at the colorful splash art on the cover, and slides it in his backpack with care. (Booleon for cool comic collected?)
    
    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== bookstack_real ==

    //the real book stack file... boring ones Nick has to read for class
    
    NICK: Oh, that book... Didn't I have to read that for class once?
    
    :NICK:
    
    * [Open the book on the top of the stack.]
        -> readBook


== readBook ==

    NICK: Huh? I don't remember this part... oh, it's probably because I only read the SparkNotes. Ugh... it's just so BORING, though. I kept falling asleep when I tried to really read it...
    
    MS SUWAN: I found that book quite gripping, myself. It's a classic for a reason.
    
    NICK: Ugh... I guess I don't really get it, then.
    
    -> END


== bookstack_dreamvisited ==

    NICK: Hey! Hold on a second - where'd the manga go?
    
    MS SUWAN: The entire stack changed...? How interesting.
    
    NICK: No way...! It's all this boring basic stuff now. What the heck?
    
    NOUR: Whoa, hey, I wouldn't call Sherlock Holmes boring or basic. The short stories are actually pretty cool. And I personally really liked Huck Finn. There's a lot you can get out of it.
    
    NICK: I mean... there's a lot you can get out of Blood Titan Online, too. I guess it just doesn't seem as - as professional as those other books, or something. Man, why'd it all change before I got a chance to even read them?
    
    MS SUWAN: I quite enjoyed the Sherlock Holmes collection, too. It's a fascinating look into the culture and commonalities of Victorian London.
    
    NOUR: Right? And some of those mysteries... phew. I wanna get into Sir Arthur Conan Doyle's head. He definitely always had some interesting things cooking up in there.
    
    NICK: ...
    
    NICK: Couldn't it have left just ONE volume of Blood Titan Online for me? Why does this mall hate me so bad?
    
    -> END


== sailormooncomic ==

    NICK: Hey Nour, I think there's a Sailor Moon comic in here somewhere... oh, there's one with Sailor Jupiter on the cover. Is she your favorite?
    
    NOUR: I'm more of a Sailor Uranus fan.
    
    NICK: Haha-
    
    NOUR: Nick, don't say it.
    
    NICK: Uran -- Uranus --
    
    NOUR: Nick, shut up!
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
