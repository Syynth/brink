== hotwheels ==

    {
        - currentWorld == "dream" && hotwheels_real:
            -> hotwheels_realvisited
            
        - currentWorld == "dream":
            -> hotwheels_dream
            
        - currentWorld == "real" && hotwheels_dream:
            -> hotwheels_dreamvisited
        
        - else:
            -> hotwheels_real
    }
//---------------- DREAM WORLD ----------------

== hotwheels_dream ==
    
    //Should have an initial interaction in the dream world pointing out how new this looks, with Ms. Suwan vividly remembering being given Mattel cars as a child as gifts from local toy markets. She has a uneasy feeling about these toys and the type of media she was “given” to play rather than chosen by herself. Nick can speak up about how he thinks his friends would think the car is really cool, and they can have a conversation about it.
    
    A shiny toy race car. Glimmering in the light as if it was freshly made.
    
    Ms Suwan stares down at it as she forms a crooked grin.
    
    NICK: Omg I used to play with those all the time. My go to was the '20 Dodge Charger Hellcat. 
    
    MS SUWAN: I played with them too, The 1970 Mattel Hot Wheels Side kick Thailand SC Police Diecast Car was the one I would play with the most.
    
    NICK: You had Hot Wheels in Thailand?!?
    
    NOUR: Bro there are American toys everywhere not just in America.
    
    MS SUWAN:  Haha yes, My mother would buy them for me all of the time at the local toy stores. I never even asked for them but she still bought them for me. It was the only toy I had to play with for most of my childhood.
    
    NICK: Really? I didn't take you as the type to play with toy cars.
    
    NOUR: Nick stop.
    
    Ms.Suwan places the toy back down on the table. Ready to leave it behind for good.
    
    She walks away feeling content, knowing that she can't continue being reminded of her past.
    
    -> END


== hotwheels_realvisited ==

    The shiny toy race car. Ms Suwan can see herself in the reflection.
    
    Ms Suwan stares down at it as she forms a crooked grin.
    
    NICK: It looks super shiny now.
    
    MS SUWAN: Yes,it does. Reminds me of how it looked in the street markets back in Thailand.
    
    NICK: So, why did you never play with them
    
    NOUR: You already asked her that
    
    MS SUWAN: It's alright. My mother would buy them for me all of the time at the local toy stores. I never even asked for them but she still bought them for me. It was the only toy I had to play with for most of my childhood but I didn't like too.
    
    NICK: Im sorry.
    
    NOUR: Why don't we look at something else.
    
    Ms Suwan places the toy back down on the table. Ready to leave it behind for good.
    
    She walks away feeling content, knowing that she can't continue being reminded of her past.
    
    -> END


//SUBSEQUENT DREAM KNOTS HERE


//---------------- REAL WORLD ----------------

== hotwheels_real ==

    A minature toy of an American car. The once bright red paint has now faded into a light brown color. 
    
    NICK: That looks like vintage hot wheels
    
    NOUR: Nick just because something is old doesn't make it vintage
    
    NICK: Just look at it, its all rusty. It has to be at LEAST 40 years old. I can't believe they used to sell them here.
    
    MS SUWAN: Old toys like these were very popular in the 80s. 
    
    NICK: Oh did you like to play with these toys when you were a kid?
    
    MS SUWAN: No
    
    NICK: Why not?
    
    NOUR: Nick
    
    NICK: What
    
    MS SUWAN: I simply did not play with them as a kid. 
    
    NICK: All of my friends had them and they let me play with them when we hung out.
    
    NICK: Hey Ms Suwan, can you pass me the toy car?
    
    MS SUWAN: Um, ok
    
    :MS SUWAN:

    + pick it up
    
        -> hotwheelsshift
        
    + walk away
    
        -> DONE
    
    =hotwheelsshift
    
        &Ms. Suwan picks up the car.
        
        -> DONE


== hotwheels_dreamvisited ==

    The hot wheels toy you saw before...
    
    -> END

//SUBSEQUENT REAL KNOTS HERE
