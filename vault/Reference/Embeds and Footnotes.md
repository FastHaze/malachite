---
tags: [reference, test]
---
# Embeds and Footnotes

## A picture

On a line of its own, a picture is shown where it is embedded:

![[hills.png]]

At half the width, with `|240` after the name:

![[hills.png|240]]

The Markdown way of writing it works too:

![Hills at 120|120](Attachments/hills.png)

## A note inside a note

The whole of another note:

![[Lentil Soup]]

Only what is under one of its headings:

![[Sourdough Bread#Quantities]]

Only one block, the line marked `^no-cms` in the design brief:

![[Design Brief#^no-cms]]

## Footnotes

Bread needs time more than it needs work.[^time] The soup is quicker.[^soup]

A footnote can be named with a word as well as a number.[^named]

[^time]: Most of the day in [[Sourdough Bread]] is waiting.
[^soup]: Forty minutes, says [[Lentil Soup]].
[^named]: Its mark still shows as a number, in the order the marks come.

## What is not shown

![[A note that is not there]]

![[scan.pdf]]
