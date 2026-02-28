== dishwasher
"Yes?" The young man looked up.
"I'm the detective in charge. Can I ask you some questions?"
"Sure. I'm Dan. What do you want to talk about?"
I should probably ask him about last night.
Finding out more about him would also be a good idea.
->talk

= talk

+ [DAN\|DANIEL\|YOU\|YOURSELF\|DISHWASHER]
"Who are you?"
"Daniel Jones. I wash the dishes here."
"By hand?"
"Nick didn't like technology. 'My ice cream is ART,' he said, 'machines can't make ART!'"
"How old are you?"
"Nineteen. Been working here two years."
"And on your spare time, you..."
"Not a lot of spare time in this business."

+ [LAST NIGHT\|YESTERDAY]
"So what happened here last night?"
"After we closed, I locked up the place. After doing that, I normally do the dishes."
"Normally?"
"Yeah... I didn't want to go to the kitchen last night."
"Why not?"
"Because of the fight." #sound
~ Knowledge += fight
"What fight?"
"Nick was yelling at Aspen. The chef."
"About what?"
"Aspen had resigned. He felt betrayed."
"Did you hear what they were yelling?"
"He said her cooking was mediocre, and that she was lucky to have a job here. Then I think she threw a plate at him."
"Hmm. Anything else?"
"You should probably ask her."
"You're right. What did you do next?"
"I went home. I figured I could return and do the rest of the dishes today. Which is how..."
"You found the body?"
"Yeah."
"Did anyone see you last night?"
"I went straight to bed. But I talked to a neighbor when I got home."
"Thanks."

+ [VICTIM\|SNOW\|NICK\|NICK SNOW]
"What was Mr. Snow like?"
"He was a cool guy. I mean, he hired me."
"Okay?"
"I was a high school dropout. Not a lot of people believed in me. He had faith in me. He gave me a job."
"... as a dishwasher, though."
"Letting you into his kitchen was the biggest compliment that guy could give anyone."
"Did you like the job?"
"Yeah. I was good at it. Paid well, too."
"I'm guessing you had no reason to kill him, then."
"Is that a trick question?"
"Heh."
"Honestly, I have no idea what to do now. I won't find another job like this."
"You think the parlor will close?"
"Nick was the only one who knew his secret recipe."
"I see. Thanks."

+ [DISHES]
"You're doing the dishes now?"
"Yeah."
"Wouldn't you normally do those last night?"
"Yeah, I... couldn't."
"Oh? Were you busy?"
"I couldn't get into the kitchen."
{Knowledge !? fight: 
	I should REALLY ask him about last night.
- else:
	"Hmm..."
}

+ [FREEZER\|KEY]
"Is the freezer normally locked?"
"Yeah. At least at night."
"Not during the day?"
"Aspen needs to access it."
"And she doesn't have a key?"
"Nah. Only Nick does. He leaves the key in the kitchen when we're open."
"Was the freezer unlocked when you left last night?"
"I'm pretty sure it wasn't."

+ [BYE\|LEAVE]
"Thanks. This was helpful."
"Glad I could help."
->kitchen

- ->talk
