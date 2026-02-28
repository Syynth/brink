=== a_tale_of_christmas_present===
VAR a_christmas_venture = 0
VAR coffee_name = 0
VAR coffee_money = 100
VAR coffee_beans = 0
VAR coffee_advertisement = 0
VAR coffee_venue = 0
-> christmas_present_egress

- (christmas_present_egress)
Coffee At Christmas # CLASS: event

- It is Christmas in the Neath. The streets are filled with snow. Urchins slide down Watchmaker Hill on handmade sleds. The Benthic and Summerset Colleges have put their feud on hold in the spirit of the festivities. There are even rumours of Masters making door to door visits.
    One snowy morning you have a most fabulous idea!
What is it that people want more than anything on dreary days such as these? It gets quite cold this time of year, and you are sure many people would love a nice cup of Darkdrop Coffee to warm their soul...or lack-thereof.
+   You make plans immmediately.
+   Oh come now, I am quite comfortable in my abode. Why should I make haste?
        Well, where there is opportunity, there is profit! Maybe you can use this opportunity to make some money, or strengthen connections, or out of the good-will of your heart?
        ++  Do it for Wealth!
                Nothing wrong with lining your pockets.
        ++  Do it to strengthen your Network.
                You'll be suprised what people let slip when they find the smallest bit of respite.
        ++  Do it out of the kindness of your Heart.
                Just think of the children!
        ++  Do it in aid of the Revolution!
                You make logical deductions and leaps of faith until you come to a most illogical conclusion of how selling Coffee will benefit the Liberation.
                
- Firstly, and maybe the most important business!
This venture must have a name.
    +   Snowbucks
            ~ coffee_name = "Snowbucks"
            The name reminds you of the Zee. Perhaps you'll get a Drownie to pose for the logo.
    +   Bazaar Brews & Beans
             ~ coffee_name = "Bazaar Brews & Beans"
             Perhaps invoking the Bazaar will garner you some respect.
    +   Echo Espresso Emporium
             ~ coffee_name = "Echo Espresso Emporium"
             Only an Echo a cup!
    +   Nadir Nectars & Coffees
             ~ coffee_name = "Nadir Nectars & Coffees"
            You'll forget you've had anything better.
    +   The Rubbery Roasts
             ~ coffee_name = "The Rubbery Roasts"
            Perfect to go with your Rubbery Lumps!
    +   Parabolan Percolations
             ~ coffee_name = "Parabolan Percolations"
             The stuff dreams are made of.
    +   The Boatman's Brew
             ~ coffee_name = "The Boatman's Brew"
             You doubt that Skeleton will complain in person, but perhaps be ready for a confrontation upon your next death.
-
    +   [Gather what you need.]
- (opening_a_shop) 
# CLEAR
{coffee_name} will need supplies, advertisers, and a venue to sell your Darkdrop Coffee. Best get to it!
You currently have {coffee_money} Echoes in Savings. # CLASS: italics
{coffee_beans: You will be brewing {coffee_beans} # CLASS: italics }
{coffee_advertisement: You will let London know with {coffee_advertisement} # CLASS: italics }
{coffee_venue: Your venue is {coffee_venue} # CLASS: italics }
    +   {coffee_beans == 0} [Acquire Coffee Beans]
        You head down to the Bazaar Shops to find a Sack of Darkdrop Coffee Beans. The smell of Spices and wet metal fills your nostrils as you pass stall after stall of curious wares.
        You find a stall selling the Darkdrop Coffee Beans, but at an absurd price! 30 Echoes for a single sack! You have other expenses to consider! And besides, you aren't even sure you will make a profit.
        The Opportunistic Rat races over from between bags of apples and barley.
        "Oye Mate! You looks like you want something, but for less than theys want to give it te ya. I thinks maybe we can comes to an arrangement."
            ++   Dismiss the Rat, you will not use illicit means to get high quality ingredients! (30 Echoes)
                    You wave the Rat off, he goes to be a nuissance elsewheres. You engage in commerce with the shopkeeper and walk away with Premium Quality Darkdrop Beans.
                    ~ coffee_beans = "A Premium Bag of Dark Drop Coffee Beans"
                    You will serve {coffee_beans}. # CLASS: italics
                    ~ coffee_money = coffee_money - 30
                    +++ [Proceed...]
                    -> opening_a_shop
            ++   Engage with the Rat, perhaps he can obtain what you require for less. (20 Echoes)
                    You takes your money and scampers away. You wait a minute, then two, then five. You are beginning to wonder if the Rat had tricked you.
                    Suddenly the Opportunistic Rat reappears, holding what appears to be his prize.
                    "Alright, alright, alright. Wasn't no problem not one bit. Just had my friend give this old hag a bite while I grabbed the goods. I could only grab one of the ones in the back though.
                    He hands over the bag, and scampers off.
                    You open the bag to inspect your prize. The beans are of a lower quality than you had hoped, but they will suffice. You close the bag, and maybe feel a little bad for the acquisition.
                    ~ coffee_beans = "A Bag of Lower-Quality Dark Drop Coffee Beans, Immorally Acquired"
                    You will serve {coffee_beans}. # CLASS: italics
                    ~ coffee_money = coffee_money - 20
                    +++ [Proceed...]
                -> opening_a_shop
            ++   Engage with the Rat, enquire if he has something similar...for a lesser price. (10 Echoes)
                    The Rat eyes you warily.
                    "Alright Govn'r! Follows me."
                    He leads you down a back-alley to an abandoned building and tells you to wait for a minute or two. He scampers in the wall, you here the skittering of paws and the tiny raspings of conversation, before the Opportunistic Rat and several other rats push a sack out a window.
                    You open the bag to find small, round, brown pellets of an unknown variety. They have the putrid smell similar to Darkdrop Coffee Beans.
                    You exchange the meager fee, and the Opportunistic Rat thanks you for your cleaning services.
                    ~ coffee_beans = "What Appears to be Coffee Beans which assuredly did not come out of a Rat"
                    You will serve {coffee_beans}. # CLASS: italics
                    ~ coffee_money = coffee_money - 10
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Consider Other Matters
                -> opening_a_shop
    +   {coffee_advertisement == 0} [Engage Advertisers]
        You consider the many various options for advertising your campaign, and come up with a few contenders.
        First, you could have the Naga Advertising Agency create a campaign worth of your venture.
        Or, you could reach out to local contacts on Doubt Street.
        Finally, you could rely on good-old word of mouth...along with a donation to a few reputable Urchins to secure
            ++  Engage with the Naga Advertising Agency to create out a full campaign! (30 Echoes)
                The Naga Advertising Agency puts out all the stops. Posters across city walls, discussions at Society parties, even a smeer campaign against any potential rivals.
                You receive inquires from many factions of London; most notably a single card:
                "Indubitably, I harbor a fervent aspiration to procure a reservation at the impending grandiloquent unveiling of your venerable emporium. Eagerly anticipating the imminent convocation, where the resplendent amalgamation of ambrosial brews and gastronomic delights shall undoubtedly transpire, I beseech thee to secure a coveted seat amidst the opulent opus of your coffee soirée." # CLASS: italics
                    ~ coffee_advertisement = "A Most Impressive Ad Campaign Across All of London"
                    You now have {coffee_advertisement}. # CLASS: italics
                    ~ coffee_money = coffee_money - 30
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Put out an ad and article in a local paper. (20 Echoes)
                The article is concise and to the point. It doubly, triply states that anyone who is anyone will have to attend the grand opening of {coffee_name}. You hope this will be enough.
                    ~ coffee_advertisement = "An Ad in a Doubt Street Publication"
                    You now have {coffee_advertisement}. # CLASS: italics
                    ~ coffee_money = coffee_money - 20
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Have Urchins Spread the Word, with a little to grease the wheels. (10 Echoes)
                Do not underestimate the power of this vocal network of roof-jumping vagrants. You give your contacts enough weasels to at least have them consider spreading the word of the opening of {coffee_name}.
                    ~ coffee_advertisement = "Rumours Spread By Urchins, some of which might be true"
                    You now have {coffee_advertisement}. # CLASS: italics
                    ~ coffee_money = coffee_money - 10
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Consider Other Matters
                -> opening_a_shop
    +   {coffee_venue == 0} [Procure a Venue]
        You'll need a space to sell your wares, and a place for your patrons to enjoy their beverages. Think of the Ambiance!
            ++  Find the means to have your Grand Opening in Parabola (30 Echoes)
                You purchase barrels of Prisoner's Honey. So much so you have to fill out several long-winded legal documents to even complete the exchange. With these barrels, you will be able to lead the finest guests into Parabola, where they will dine within Dreams and taste the untasteable. They say location is everything don't they?
                    ~ coffee_venue = "Parabola"
                    You will dine in {coffee_venue}. # CLASS: italics
                    ~ coffee_money = coffee_money - 30
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Use your connections to host the Opening on your ship (20 Echoes)
                You must purchase dining sets and tables, train a crew of motley sailors into civilized waiters and servers, as well as plot a course that both showcases the wonders of the Zee, without getting to close to the dangers that lie beyond.
                    ~ coffee_venue = "Your Ship"
                    You will dine in {coffee_venue}. # CLASS: italics
                    ~ coffee_money = coffee_money - 20
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Repurpose your stall from the Bone Market (10 Echoes)
                Maybe this is a frugal choice - or maybe this is choice of ambiance. In any case, you hope your patrons won't mind drinking alongside the remnants of your creations. And who knows...maybe this will be good advertising for your skeleton business!
                    ~ coffee_venue = "The Bone Market"
                    You will dine in {coffee_venue}. # CLASS: italics
                    ~ coffee_money = coffee_money - 10
                    +++ [Proceed...]
                    -> opening_a_shop
            ++  Consider Other Matters
                -> opening_a_shop
    + {coffee_beans != 0 and coffee_advertisement != 0 and coffee_venue != 0} Prepare for the Grand Opening
    -> the_grand_opening

- (the_grand_opening)
# CLEAR
- You are ready, or as ready as you can be. You feel the sweat bead in your palms. Your footsteps feel lighter, not entirely in part due to your lightened wallet. <>

{coffee_money == 70: It cost you the bare minimum.}
{coffee_money == 60: You barely spent a cent.}
{coffee_money == 50: You still have half of your savings remaining.}
{coffee_money == 40: You spent a fair amount.}
{coffee_money == 30: You spent most of your money.}
{coffee_money == 20: Nearly everything is top-notch.}
{coffee_money == 10: No expense was spared.}

+ [View Your Venue]

- {coffee_name} has taken up residence <>

{coffee_venue == "Parabola":
in {coffee_venue}. Although many people have trodden into Parabola while dreaming; it is the stark majority that have never truly entered it through the mirror. You hired several Silverers to help bring the customers in...and to make sure that they don't wander off into the forests of dreams and nightmares beyond the security of your Base-Camp.
}
{coffee_venue == "Your Ship":
on {coffee_venue}. You have planned a most elegant pleasure cruise around the more safe shipping lanes of the Zee. You will serve biscuits, scones, and other delights to match the atmosphere of the various Zee Regions. Hot drinks will be served in the frigid North; spiced treats when approaching the Khanate, etc. A careful application of the Treacheries of Clocks and Maps will make the journey much more expedited than one might assume.
}
{coffee_venue == "The Bone Market":
in {coffee_venue}. Well you already had a venue didn't you? Besides, just put a few bone-chairs together, finally find a place for that chandalier made of ivory humerus', and craft together a magnificent arch made of the headless skeletons of your most legal acquisition. It didn't cost much; but sometimes, minimalism is best.
}
+ [Consider the Crowds]

- Due to the advertisements of <>

{coffee_advertisement == "A Most Impressive Ad Campaign Across All of London":
{coffee_advertisement}, you enjoy the company of only London's finest crust. This is the cream of the crop, the gilded, the extravagent. Yes, anyone who is anyone had to be here...and because of that, it had to be even more exclusive. You recieved multiple requests for reservations from the most esteemed of clubs, and even a most elegant letter from the Shuttered Palace.
In one corner sits the Clay Tailor, looking most dapper, in another corner, the Mr. Slowcakes Amensius!
And in the back, in the most private and solitary room you can find, is a single chair(enlarged and shaped specifically to size) reserved for Mr. Pages.
}
{coffee_advertisement == "An Ad in a Doubt Street Publication":
{coffee_advertisement}, you partake of the company of the common man of London! It is an open party, let it be known that you turn away no man. Constables dine with Rubbery Men, Bohemians discuss the arts with the colleges of the University, the Benthic and Summersets argue scholarship with the Bohemians (you can't rightfully tell if anyone is actually listening to each other).
This is the beating heart of London.
}
{coffee_advertisement == "Rumours Spread By Urchins, some of which might be true":
{coffee_advertisement}, you had a ripe showing of the more nefarious elements of London's society. This is a crowd that a more respectable or socially conscious sort of person would be careful to avoid. But because of this, the people are true to their core. Zailors pile in after a hard days work on the Docks, the Duchess arrives in a most inauspicious carriage, and even the Topsy King makes an appearance! Best to keep them apart; criminal empires and all that.
}
+ [Begin preparing the coffee]

- You have procured the finest ingredients. Guests will be served a hot piping beverage of Coffee. Made from <>

{coffee_beans == "A Premium Bag of Dark Drop Coffee Beans":
{coffee_beans}. This is only the finest selection of ground coffee beans you could muster. You partook of every step of the preparation process. You conscripted the help of some university fellows to design an apparatus to extract only the most delectable fluids from the beans, and infused with such spices and flavours that the drink itself will be held in the same esteem as Year of the Turtle.
}
{coffee_beans == "A Bag of Lower-Quality Dark Drop Coffee Beans, Immorally Acquired":
{coffee_beans}. There are practices out in Station VIII that enable the scientifically-minded to extract the essence of an object. It isn't so much about the coffee flavour that you are going for, rather, for the extraction of the emotions.
From these beans you extract "Weariness of the World"; a potent brew, one who drinks it is engrossed in 'Weltschmerz'.
For the non-Germans in the audience, you explain how the drink imbues the psychological pain caused by sadness that can occur when realizing that someone's own weaknesses are caused by the inappropriateness and cruelty of the world and (physical and social) circumstances.
The effect is temporary, a wave of sadness and misery that overflows on the drinker. But when they finally clear it from their body, they will feel a sense of euphoria as they find new joys in their life.
}
{coffee_beans == "What Appears to be Coffee Beans which assuredly did not come out of a Rat":
{coffee_beans}. Cruel. Terribly cruel. Perhaps you did this to spite a certain individual, perhaps you just like to watch the world burn. The smell is ghastly, and flows over the members of your event. About half of those who attend will never get the smell out of their clothing (which, from a certain point of view is quite charitable, as they end up donating it to the local Orphanage), the other half will get the smell out(but opt not to wear that overcoat quite as frequently).
}

+ [Continue]

- (did_they_like_it)
# CLEAR
Reviews will pile in during the following days.
Some will discuss impressivity of your venue, others how chique it was.
Some will say that the company was marvelous, others will gossip for weeks after about your lack of character for inviting such charletons.
Some will marvel at the creativity and imagination that was necessary for such marvelous tastes to dance upon their palates; others will say your drinks tasted like dogwash.
In essence, they will all speak of your merits and demirts, but take no actions to create a better world. Such is the role of a critic, you muse.
+ [...]
-
You finish packing up the venue, and head back to your lodgings.
+ [Go for a walk]
-
As you are walking home, you feel a most unusual and strange sense of emotion overwhelm you. Maybe it is the romantic falling of snow, perhaps it is the hushed way two lovers hold hands, or maybe the lacre is getting to you. In any case, any idea worms itself into your thought. Overpowering all other senses.
+ [Engage in Charity]
{coffee_money <= 40: With your measly savings, engage in charity| with your impressive savings, engage in charity}
-> coffee_epilogue

- (coffee_epilogue)

You take what remains of your coffers, {coffee_money} Echoes, and purchase some imported chocolate powder. A delicacy on the Surface, even more so in the Neath.
You drag the tools from your coffee venture and set up a small table outside St. Dustin's Church. The guano-covered gargoyles gaze down upon you; as if awaiting a moment to strike.
+ [Hail a passing Urchin.]

- You buy a lump of coal from a passing Urchin, and draw, in quite crude letters, {coffee_name} on a small signpost. Until the last drop of hot chocolate is gone, you shall stand sentinal outside the church.
+ [Open your makeshift stand]

# CLEAR
- (the_stall)
{the_stall:
    - 1: You stand alone. # CLASS: cosmogone
    - 2: Someone has joined you. # CLASS: cosmogone
    - 3: A line begins to form for your Hot Chocolate. # CLASS: cosmogone
    - 4: Citizens begin lingering around your stall. # CLASS: cosmogone
    - 5: Warring factions share warmth under the chapel. # CLASS: cosmogone
    - 6: Urchins begin to build snowmen out of lacre and snow. # CLASS: cosmogone
    - 7: There is a hum, it throbs, it pulses, it breathes. There is life in this dark place. # CLASS: cosmogone
    - else: This is only a small fraction of the citizens of London; but the smiles are real, and warmth, like sunlight, spills upon the street. # CLASS: cosmogone
}
Someone approaches your stall... # CLASS: italics
    * The Bohemians [...] will delight in the hedonism of the brew.
        -> the_stall
    * The Church  [...] will respect your charity.
        -> the_stall
    * The Urchins  [...] will bounce in joy from the taste, which you offer at a discount for their services in aiding you.
        -> the_stall
    * A passing devil  [...] will pass by, sniffing the air as he passes; and it will look as if a memory of a pleasant past has just crossed his brow.
        -> the_stall
    * Members of all factions of London  [...] , hidden and in plain sight, will eventually approach your stall.
        -> the_stall
    * A Dockworker  [...] will spend his bonus on several cups, and hand it out to some Rubbery Men who were too shy to approach your stall.
        -> the_stall
    *   A pack of rats  [...] will scrounge together and purchase a single cup to share between the lot of them.
        Some Cats will follow suit as well.
        -> the_stall
    *   {the_stall == 8} Look Upon Your Work, Ye Mighty[...], and breath a sigh of relief.
        -> open_stall

- (open_stall)
You will make no profits from this endeavour. You will make no friends. By tomorrow eve, everyone will have forgotten about your charity. But for today, this is enough.
For we are all adrift on a sea of misery, and it is only by aiding each other, in large and small ways, that we even have a chance of staying afloat. # CLASS: cosmogone
Merry Neathmas, and Happy New Year. 1899 was a wonderful year; here is to another wonderful 1899! # CLASS: italics

{end_tale(a_christmas_venture)}
+ The End []
    -> your_lodgings