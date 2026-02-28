== chef
I sat down next to the chef.
"Thanks for coming down here."
"Of course. How can I help?"
I should probably ask her about last night, and what she knew about the victim.
{Knowledge ? fight: I should ask about that fight, and her resignation, too.}
->talk

= talk

+ [LAST NIGHT\|YESTERDAY\|ALIBI]
"What were you doing last night?"
"When do you mean? After work?"
"Let's start there."
"At a nightclub. The Icecube."
"I know of it. Were you alone?"
"No. It was a friend's birthday party."
"When did you leave?"
"Around 4 AM."
"When did you leave work?"
"Shortly after closing. 9:15, perhaps."
"And what was Snow's state at that time?"
"He was very much alive."
"Was anyone else here when you left?"
"I don't think so. Crystal was out, and Dan had already left."
"So you may have been the last person to see the victim alive."
"Well, I'm sure the last person to see him alive was the murderer."

+ [VICTIM\|SNOW\|NICK\|NICK SNOW]
"Did you know Nick Snow well?"
"He's been my boss for eleven years."
"Was he a good employer?"
"Is there such a thing?"
"I would assume so."
"We had our differences."
"Can you give me an example?"
"He was very self-centered."
"I guess that's normal when you build a multi-million dollar ice cream empire."
"Yeah, but nobody does that on their own. He's had a lot of help. And he's not the best at acknowledging that."
"You didn't feel seen?"
"I don't think he appreciated how much I contributed here, no."
"So you weren't friends, then?"
"I had no problems working in the same kitchen as him. But he's not the kind of person I would have spent time with after work."
"I see."

+ (workConversation) {Knowledge ? fight} [JOB\|WORK\|RESIGNATION\|QUITTING\|RESIGNING]
"You'd resigned from this job?"
"You know about that?"
"I know everything."
"Yeah. I'm starting a new job."
"When did you plan to leave?"
"I gave Nick my three month notice yesterday."
"You're going to another restaurant?"
"Not exactly. I'm starting a business."
"What kind of business?"
"Do you drink bottled water?"
"Occasionally."
Aspen pulled a bottle of water out of her bag and gave it to me. It felt cold. #sound
~ Inventory += water
"This is no ordinary water."
"It's not?"
"It contains all the vitamins and calories you need in a day."
"Really?"
"And its chemical balance keeps it cool and refreshing. Keep it in your bag for an hour on a hot summer day, and it's still like drinking ice water."
"What does it taste?"
"We have a wide range of flavors. Anything from raspberry milkshake to a full pot roast dinner. That bottle right there tastes like cauliflower."
"You think people will buy this?"
"That's what all our market research shows. My investor has twenty years of experience, and he's convinced it will be a hit. We're aiming for two hundred grocery stores by Christmas."
{not fightConversation: So this is what she and Snow was fighting over last night. I should probably ask her about that.}

+ (fightConversation) {Knowledge ? fight} [FIGHT]
"Were you and the victim fighting last night?"
"As a matter of fact, yes."
"About what?"
"I gave him my resignation. He didn't take it well."
"Why not?"
"I think he felt betrayed. 'I will not let you leave!' he shouted."
"'Let you'?"
"He believed there was a clause in my NDA that gave him the right to stop me from working in the food business for two years after quitting here."
"Is there?"
"Of course not. And I told him that."
"And then?"
"He said 'You're not even that good a chef anyway!"
"That's when you threw a plate at him?"
"... yeah."
{not workConversation: I should probably ask what her new job is.}

+ [BYE\|LEAVE]
"Thanks for talking to me."
"It's not like I have a choice."
She started scrolling on her phone again the second I got up from the table.
->restaurant

- 

{workConversation && fightConversation && Knowledge !? mirror:
    I jotted down what I'd just learned on my notepad.
    "Thank you for your coop..."
    "Wait. You don't suspect me, do you?"
    "..."
    "I've known him for eleven years! Just because I fought with Nick doesn't mean I killed him!"
    "But you do have a motive."
    "A pretty weak one, I'd say."
    "Oh? You had a brilliant business idea that would make you rich, and he was in your way."
    "He couldn't stop me. My NDA didn't..."
    "But he could stall you in the courts for years."
    "I guess..."
    "And you were apparently the last person to see him alive."
    "Yeah, but compared to certain other people, that¨s a pretty weak motive."
    "Oh? Who would that be?"
    "..."
    "..."
    "I didn't want to tell you this. I figured it was none of your business. But I'm not going to jail protecting someone else."
    "Who?"
    "I have some photos you should see."
    "Show me."
    "Not on me. They're hidden."
    "Where?"
    "You see that restroom over there?"
    "Yeah?"
    "I suggest you go in there, and... shake the mirror a bit. Okay?" #sound
    ~ Knowledge += mirror
    "Why?"
    "Trust me. There's your motive."
    "I'll check it out."
}
->talk