# `sURE`

`sURE` stands for `scrape Utah Real Estate`.

When built with live credentials, this will acquire a URE session id and,
using configured url-encoded parameters, will query URE's API for the
given county, parse the active listings with a "Just Listed" designation,
and using Twilio, will text configured phone numbers with a list of 
the "Just Listed" listings.

At a minimum, `sURE` will connect with URE's servers twice:
once to get a session id, and once to hit the listings API.

At a maximum, it will connect 2+_n_ times, where _n_ is equal to the number
of listings found.

It will store any previously scraped listings' MLS ids in a file, `listings.txt`,
and will cross-reference the MLS ids acquired in the API call against those
already checked, further preventing any unnecessary connections to URE's
servers.

## Configuration

Configuration files will live in `~/.sure`. You'll need to create this
directory before running the script for the first time.

It'll need two files: `twilio.env` and `queries.env`. In the following
formats:

### `twilio.env`

```env
AccountSID=TWILIO_SID_VALUE
AuthToken=TWILIO_AUTH_TOKEN
TwilioNumber=THE_TWILIO_NUMBER_YOUVE_PURCHASED
AlertNumbers=COMMA_SEPARATED_LIST_OF_VERIFIED_RECIPIENT_PHONE_NUMBERS
```
Make sure the phone numbers are formatted `+11234567890` for single 
numbers, and `+11234567890,+11234567890` for lists.

### `queries.env`

```env
param=county_code
value=Davis
chain=saveLocation,criteriaAndCountAction,mapInlineResultsAction
tx=true
all=1
accuracy=100
geocoded=Davis
state=UT
box=%257B%2522north%2522%253A41.153644%252C%2522south%2522%253A40.77345%252C%2522east%2522%253A-111.7385751%252C%2522west%2522%253A-112.4933929%257D
lat=40.9628845
lng=-112.0953297
selected_listno=
type=1
geolocation=Davis+County%2C+UT
listprice1=
listprice2=420000
tot_bed1=3
tot_bath1=2
stat=7
status=1%2C7
opens=
o_env_certification=32
proptype=1
style=
o_style=4
tot_sqf1=
dim_acres1=
yearblt1=
cap_garage1=
o_has_solar=1
o_seniorcommunity=1
o_has_hoa=1
o_accessibility=32
htype=county
hval=Davis
loc=Davis%20County,%20UT
accr=100
advanced_search=0
param_reset=housenum,dir_pre,street,streettype,dir_post,city,county_code,zip,area,subdivision,quadrant,unitnbr1,unitnbr2,geometry,coord_ns1,coord_ns2,coord_ew1,coord_ew2,housenum,o_dir_pre,o_street,o_streettype,o_dir_post,o_city,o_county_code,o_zip,o_area,o_subdivision,o_quadrant,o_unitnbr1,o_unitnbr2,o_geometry,o_coord_ns1,o_coord_ns2,o_coord_ew1,o_coord_ew2
```

These can be customized however you like. We don't currently url-encode
the parameters once they're parsed from this file, so it's recommended
that you paste them in pre-encoded.

