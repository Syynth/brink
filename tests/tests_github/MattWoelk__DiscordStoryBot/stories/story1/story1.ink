# author: [test story]
# hidden

VAR health = 100


Location: The great castle of ooooooom #img:castle_lowres.jpg
Residence of Monsieur Phileas Fogg.
-> downtown

== downtown
# downtown tag
It was cool downtown. #tag ya
-> suburbs

== suburbs
# suburbs tag
Suburbs were cool too. #tag too
~ health -= 2
-> london

=== london ===
# health +1
Monsieur Phileas Fogg returned home early from the Reform Club, and in a new-fangled steam-carriage, besides! #tag1 #tag2
health: "{health}"
# tag 4
"Passepartout," said he. "We are going around the world!" #tag 3

* 🧔🏿‍♂️ - nod really well
    -> nod
* ❤ - be astonished
    I was utterly astonished.
    -> astonished
* 🙂 - nod -> nod


=== astonished ===
"You are in jest!" I told him in dignified affront. "You make mock of me, Monsieur."
"I am quite serious."

* 🙁 - be sad about it
    -> ending


=== nod ===
I nodded curtly, not believing a word of it.
-> ending


=== ending
"We shall circumnavigate the globe within eighty days." He was quite calm as he proposed this wild scheme. "We leave for Paris on the 8:25. In an hour."
-> END
