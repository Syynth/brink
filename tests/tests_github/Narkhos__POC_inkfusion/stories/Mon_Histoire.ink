VAR ONE_indice = false

Vous voyez un bel indice.

* {TWO_variable} J'ai déjà joué à l'autre jeu
->END
* Ramasser l'indice
~ONE_indice=true
* Laisser l'indice

-
Qui est le coupable ?

 + C'est moi !
 Ah, zut je me suis trompé !
 PERDU
 
 + C'est vous !
{ ONE_indice == true :
        Et je peux le prouver avec cet indice !
        GAGNE
    - else:
        Mais je ne peux pas le prouver, hélas !
        PERDU
}
-
Ainsi s'achève les aventures de l'indice à ramasser.

    -> END
