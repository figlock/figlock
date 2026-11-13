# Third-party notices

## EFF large wordlist

The passphrase generator in `crates/figlock-core/src/words.txt` uses the word column of the EFF large wordlist
for random passphrases (7776 words), from https://www.eff.org/deeplinks/2016/07/new-wordlists-random-passphrases.
Copyright Electronic Frontier Foundation, licensed under
[Creative Commons Attribution 3.0 United States](https://creativecommons.org/licenses/by/3.0/us/). The dice
numbers were removed; the words are otherwise unchanged.

## SLIP-0039 wordlist and test vectors

`crates/figlock-core/src/slip39_words.txt` is the 1024 word list of SLIP-0039
(https://github.com/satoshilabs/slips/blob/master/slip-0039/wordlist.txt), part of that specification.
`docs/spec/vectors/slip39-official.json` is `vectors.json` from https://github.com/trezor/python-shamir-mnemonic
at commit 9fcb9c807c185e06d6269b65f3d795d3c724da3b, unchanged. Copyright 2019 SatoshiLabs, MIT license.
