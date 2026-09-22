Seems to be a sound bank that only contains a single sound event.
One of the header fields contains the same hash as the HIRC "Event" object ID later on.

```101editor
//------------------------------------------------
//--- 010 Editor v12.0.1 Binary Template
//
//      File: dt_wwise_event.bt
//   Authors: Lucas Schwiderski
//   Version: 
//   Purpose: 
//  Category: Darktide
// File Mask: *.wwise_event
//  ID Bytes: 
//   History: 
//------------------------------------------------

// Definitions for Wwise Bank are taken from the repository.
// Copyright Alexander Lombardi

/*********************************************************************** 
Typedef's Section:
***********************************************************************/ 
typedef struct(uint32 dataLen)
{
    local uint32 i, numFiles = dataLen / 12; // 12 = 3 * uint32 entries.
    for(i = 0; i < numFiles; i++)
    {
        struct d_entry
        {
            uint32 fileId   <format=hex, comment=".wem file id">;
            uint32 dOffset  <comment="offset from start of DATA section">;
            uint32 dSize    <comment="length in bytes of .wem file">;
        } a_wem_file;
    }
} data_DIDX;

typedef struct(data_DIDX &DIDX_dataObj)
{
    local uint32 i;
    local const uint64 pos = FTell();

    for(i = 0; i < DIDX_dataObj.numFiles; i++)
    {
        FSeek(pos + DIDX_dataObj.a_wem_file[i].dOffset);
        struct d_entry_data(data_DIDX &DIDX_dataObj)
        {
            byte rawdata[DIDX_dataObj.a_wem_file[i].dSize] <comment="The .wem file with the length as given in the DIDX section, and starting with 52 49 46 46 -- RIFF.">;
        } aFileData (DIDX_dataObj);
    }
    FSeek(pos + parentof(this).secLen);
} data_DATA;

typedef enum <byte> {
    Settings = 1,
    SoundVoice,
    EventAction,
    Event,
    RandomContainer,
    SwitchContainer,
    ActorMixer,
    AudioBus,
    BlendContainer,
    MusicSegment,
    MusicTrack,
    MusicSwitchContainer,
    MusicPlaylistContainer,
    Attenuation,
    DialogueEvent,
    MotionBus,
    MotionFX,
    Effect,
    AuxiliaryBus = 20
} data_HIRC_object_type;

typedef struct {
    data_HIRC_object_type object_type;
    uint32 object_length;
    uint32 id <format=hex>;
    byte data[object_length-4];
} data_HIRC_object;

typedef struct {
    uint32 object_count;
    data_HIRC_object objects[object_count] <optimize=false>;
} data_HIRC;

typedef struct
{
    char secName[4];

    if(Strcmp(secName, "ffff") != 0) {
        uint32 secLen <comment="length of section">;
        if(secLen > 0)
        {
           if(Strcmp(secName, "DIDX") == 0)
            {
                data_DIDX data(secLen);
                parentof(this).flag_DIDX_defined = TRUE;
                parentof(this).idxOf_DIDX_sec = parentof(this).i;
            }
            else if(Strcmp(secName, "DATA") == 0 && parentof(this).flag_DIDX_defined)
                data_DATA data(parentof(this).aSection[parentof(this).idxOf_DIDX_sec].data);
            else if(Strcmp(secName, "HIRC") == 0)
                data_HIRC data;
            else
                byte data[secLen];
        };
    };
} section <read=getSecName>;

/*********************************************************************** 
File Definition:
***********************************************************************/ 

uint32 version <format=hex>;
Assert(version == 0x1b);
uint32 length;

// A section of values that don't belong the actual Wwise bank
struct {
    uint32 unknown1;
    Assert(unknown1 == 0x0);
    // Matches the file itself
    uint64 this_file <format=hex>;
    uint32 unknown2;
    Assert(unknown2 == 0x40);
    uint32 unknown3;
    Assert(unknown3 == 0x0);

    // The following hash-like values will probably define some of these values:
    // - name of the event
    // - Wwise bus the event runs on

    // Corresponds to the `id` value of a HIRC object of type "event" later on in the file
    uint32 hirc_event_id <format=hex, comment="Looks like a short hash">;

    // No particularly clear value.
    // Sometimes when interpreted as `float`, it produces whole numbers.
    // Sometimes, it's very close to `0xFFFFFFFF`.
    uint32 unknown5;

    // Following two usually look like hashes.
    // Also often reasonably looking floats close to `1`. Could be something like volume.
    // Would still fit with it sometimes showing up elsewhere in a `.wwise_bank`: It's not unreasonable
    // for the bnk to have volume values as well.

    // In at least one instance, shows up in `MotionBus`/`MotionFX` sections in a `.wwise_bank`.
    uint32 unknown6 <format=hex, comment="Looks like a short hash">;
    // At least in one instance, this was identical with `unknown6`
    uint32 unknown7 <format=hex, comment="Looks like a short hash">;

    uint32 unknown8;
    Assert(unknown8 == 0x0);
    uint32 unknown9;
    Assert(unknown9 == 0x0 || unknown9 == 0x1);
    uint32 unknown10;
    Assert(unknown10 == 0x0);
    uint32 unknown11;
    Assert(unknown11 == 0x0);
} header;

struct bnk
{
    local byte flag_DIDX_defined = FALSE;   // Used later to ensure DATA section is defined and declared after DIDX.
    local int32 i;                          // i is used later to find the index of the DIDX section.
    local int32 idxOf_DIDX_sec = -1;        // Will get set to i's value when DIDX gets parsed.

    for(i = 0; !FEof(); i++)
        section aSection;
} wwise_bank <open=true>;


/*********************************************************************** 
Function definitions:
***********************************************************************/
string getSecName(section &aSec)
{
    return aSec.secName;
}
```